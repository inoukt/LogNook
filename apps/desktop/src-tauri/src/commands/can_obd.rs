use super::{connect_with_diagnostics, diagnose};
use crate::AppState;
use lognook_core::{
    CanDeviceCandidate, CanDeviceSelection, CanObdAdapter, ConnectionCapabilities,
    DeviceDescriptor, available_can_adapters as list_can_adapters,
    identify_can_obd_with_sinks as identify_device,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::State;

#[derive(Default)]
pub(crate) struct CanObdOperationRegistry {
    active: Mutex<Option<Arc<AtomicBool>>>,
}

impl CanObdOperationRegistry {
    fn begin(&self) -> Result<ConnectOperation<'_>, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "CAN OBD operation state is unavailable".to_owned())?;
        if active.is_some() {
            return Err("A CAN OBD connection operation is already active".into());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *active = Some(cancelled.clone());
        Ok(ConnectOperation {
            registry: self,
            cancelled,
            released: false,
        })
    }

    pub(crate) fn cancel(&self) -> bool {
        let Ok(active) = self.active.lock() else {
            return false;
        };
        active.as_ref().is_some_and(|cancelled| {
            cancelled.store(true, Ordering::Release);
            true
        })
    }

    pub(crate) fn is_connecting(&self) -> bool {
        self.active.lock().is_ok_and(|active| active.is_some())
    }
}

struct ConnectOperation<'a> {
    registry: &'a CanObdOperationRegistry,
    cancelled: Arc<AtomicBool>,
    released: bool,
}

impl ConnectOperation<'_> {
    fn finish(&mut self) -> bool {
        let mut active = self
            .registry
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cancelled = self.cancelled.load(Ordering::Acquire);
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
        {
            *active = None;
        }
        self.released = true;
        cancelled
    }

    fn cancellation_token(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}

impl Drop for ConnectOperation<'_> {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut active = self
            .registry
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.cancelled))
        {
            *active = None;
        }
        self.released = true;
    }
}

fn adapter_id(selection: &CanDeviceSelection) -> String {
    format!(
        "candlelight-obd:{}:{}:{}",
        selection.id, selection.interface_number, selection.channel
    )
}

#[tauri::command]
pub async fn available_can_obd_adapters(
    state: State<'_, AppState>,
) -> Result<Vec<CanDeviceCandidate>, String> {
    diagnose(&state, None, "provider_discovery", vec![], |_| {
        list_can_adapters().map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn identify_can_obd(
    state: State<'_, AppState>,
    selection: CanDeviceSelection,
) -> Result<DeviceDescriptor, String> {
    identify_shared(&state, selection)
}

pub(crate) fn identify_shared(
    state: &AppState,
    selection: CanDeviceSelection,
) -> Result<DeviceDescriptor, String> {
    diagnose(
        state,
        Some(adapter_id(&selection)),
        "identify",
        vec![],
        |operation| {
            identify_device(&selection, operation.sink(), Some(state.trace_store.sink()))
                .map_err(|error| error.to_string())
        },
    )
}

#[tauri::command]
pub async fn connect_can_obd(
    state: State<'_, AppState>,
    selection: CanDeviceSelection,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    connect_shared(&state, selection, signal_ids)
}

pub(crate) fn connect_shared(
    state: &AppState,
    selection: CanDeviceSelection,
    signal_ids: Vec<String>,
) -> Result<ConnectionCapabilities, String> {
    let mut operation = state.can_obd_operations.begin()?;
    let cancelled = operation.cancellation_token();
    connect_with_diagnostics(
        &state,
        adapter_id(&selection),
        signal_ids,
        move |engine, ids| {
            let result = engine
                .connect_adapter(
                    Box::new(CanObdAdapter::with_cancel_token(selection, cancelled)),
                    ids,
                )
                .map_err(|error| error.to_string());
            if operation.finish() {
                if result.is_ok() {
                    engine.disconnect_obd().map_err(|error| error.to_string())?;
                }
                return Err("CAN OBD connection was cancelled".into());
            }
            result
        },
    )
}

#[tauri::command]
pub fn cancel_can_obd_connect(state: State<'_, AppState>) -> bool {
    state.can_obd_operations.cancel()
}

#[cfg(test)]
mod tests {
    use super::CanObdOperationRegistry;
    use std::sync::{Arc, atomic::Ordering};

    #[test]
    fn cancellation_needs_no_engine_lock_and_operation_slot_reopens() {
        let registry = CanObdOperationRegistry::default();
        let mut operation = registry.begin().unwrap();
        assert!(registry.cancel());
        assert!(operation.finish());
        drop(operation);

        assert!(!registry.cancel());
        let mut next = registry.begin().unwrap();
        assert!(!next.finish());
    }

    #[test]
    fn finished_operation_drop_cannot_clear_a_newer_connect() {
        let registry = CanObdOperationRegistry::default();
        let mut first = registry.begin().unwrap();
        assert!(!first.finish());

        let mut second = registry.begin().unwrap();
        drop(first);

        assert!(registry.is_connecting());
        assert!(registry.cancel());
        assert!(second.finish());
    }

    #[test]
    fn each_connect_uses_a_fresh_cancel_token() {
        let registry = CanObdOperationRegistry::default();
        let mut first = registry.begin().unwrap();
        let first_token = first.cancellation_token();
        assert!(!first.finish());

        let mut second = registry.begin().unwrap();
        let second_token = second.cancellation_token();
        assert!(!Arc::ptr_eq(&first_token, &second_token));
        assert!(!second_token.load(Ordering::Acquire));
        assert!(registry.cancel());
        assert!(second_token.load(Ordering::Acquire));
        assert!(!first_token.load(Ordering::Acquire));
        assert!(second.finish());
    }
}
