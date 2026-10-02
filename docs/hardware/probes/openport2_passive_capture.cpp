// Bounded OPTA/OpenPort 2 passive CAN capture.
// Build as x86. This probe binds no J2534 transmit or periodic-message function.
#include <windows.h>
#include <cstdio>
#include <cstring>
#include "C:/Program Files (x86)/OpenECU/OpenPort 2.0/samples/common/j2534_tactrix.h"

namespace {
constexpr unsigned long kReadBatch = 32;
constexpr unsigned long long kListenMs = 5000;
constexpr unsigned long long kMaxMessages = 10000;
constexpr unsigned long long kMaxBytes = 4 * 1024 * 1024;
constexpr unsigned long kSniffFlags = SNIFF_MODE | CAN_ID_BOTH;

void report_error(PF_PassThruGetLastError* get_last_error) {
    char detail[512] = {};
    const long status = get_last_error(detail);
    std::printf("PassThruGetLastError: 0x%08lX, %.*s\n", status, 511, detail);
}

bool check(const char* name, long status, PF_PassThruGetLastError* get_last_error) {
    std::printf("%s: 0x%08lX\n", name, status);
    std::fflush(stdout);
    if (status != STATUS_NOERROR) report_error(get_last_error);
    return status == STATUS_NOERROR;
}

void dump_message(unsigned long long index, const PASSTHRU_MSG& message) {
    std::printf("MSG #%llu protocol=%lu data_size=%lu extra_data_index=%lu "
                "rx_status=0x%08lX tx_flags=0x%08lX timestamp=%lu data=",
                index, message.ProtocolID, message.DataSize, message.ExtraDataIndex,
                message.RxStatus, message.TxFlags, message.Timestamp);
    if (message.DataSize > PASSTHRU_MSG_DATA_SIZE) {
        std::puts("<invalid-size; payload not read>");
        return;
    }
    for (unsigned long i = 0; i < message.DataSize; ++i)
        std::printf("%02X", message.Data[i]);
    std::puts("");
}
}  // namespace

int main() {
    static_assert(sizeof(void*) == 4, "The installed OPTA DLL requires an x86 process");
    static_assert(sizeof(PASSTHRU_MSG) == 4152, "Unexpected installed J2534 ABI layout");

    const wchar_t* path =
        L"C:\\Program Files (x86)\\OPTA\\Tactrix\\opta_j2534_0404_tactrix.dll";
    HMODULE dll = LoadLibraryExW(path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!dll) {
        std::printf("LoadLibraryExW failed: %lu\n", GetLastError());
        return 1;
    }

    auto open = reinterpret_cast<PF_PassThruOpen*>(GetProcAddress(dll, "PassThruOpen"));
    auto version = reinterpret_cast<PF_PassThruReadVersion*>(GetProcAddress(dll, "PassThruReadVersion"));
    auto close = reinterpret_cast<PF_PassThruClose*>(GetProcAddress(dll, "PassThruClose"));
    auto connect = reinterpret_cast<PF_PassThruConnect*>(GetProcAddress(dll, "PassThruConnect"));
    auto disconnect = reinterpret_cast<PF_PassThruDisconnect*>(GetProcAddress(dll, "PassThruDisconnect"));
    auto start_filter = reinterpret_cast<PF_PassThruStartMsgFilter*>(GetProcAddress(dll, "PassThruStartMsgFilter"));
    auto read = reinterpret_cast<PF_PassThruReadMsgs*>(GetProcAddress(dll, "PassThruReadMsgs"));
    auto get_last_error = reinterpret_cast<PF_PassThruGetLastError*>(GetProcAddress(dll, "PassThruGetLastError"));
    if (!open || !version || !close || !connect || !disconnect || !start_filter || !read || !get_last_error) {
        std::puts("Required OPTA/J2534 passive-capture export missing");
        FreeLibrary(dll);
        return 1;
    }

    std::puts("DLL: OPTA opta_j2534_0404_tactrix.dll; process: x86");
    std::puts("Mode: passive CAN sniff only; no diagnostic or transmit API loaded");
    unsigned long device = 0;
    bool device_open = false;
    bool channel_open = false;
    unsigned long channel = 0;
    bool ok = check("PassThruOpen", open(nullptr, &device), get_last_error);
    device_open = ok;

    char firmware[512] = {}, dll_version[512] = {}, api[512] = {};
    if (ok) ok = check("PassThruReadVersion",
        version(device, firmware, dll_version, api), get_last_error);
    if (ok) {
        std::printf("Firmware: %.*s\nDLL: %.*s\nAPI: %.*s\n",
            511, firmware, 511, dll_version, 511, api);
        if (std::strcmp(firmware, "main code version : 1.17.4877") != 0 ||
            std::strcmp(api, "04.04") != 0) {
            std::puts("Unexpected OPTA identity; stopping before vehicle channel open");
            ok = false;
        }
    }

    if (ok) {
        std::printf("Connect: CAN5 flags=0x%08lX bit_rate=500000\n", kSniffFlags);
        ok = check("PassThruConnect",
            connect(device, CAN, kSniffFlags, 500000, &channel), get_last_error);
        channel_open = ok;
    }
    if (ok) {
        PASSTHRU_MSG mask = {}, pattern = {};
        mask.ProtocolID = pattern.ProtocolID = CAN;
        mask.DataSize = pattern.DataSize = 4;
        unsigned long filter_id = 0;
        ok = check("PassThruStartMsgFilter(PASS_FILTER; zero mask/pattern; no flow control)",
            start_filter(channel, PASS_FILTER, &mask, &pattern, nullptr, &filter_id), get_last_error);
        if (ok) std::printf("Filter ID: %lu\n", filter_id);
    }

    unsigned long long total_messages = 0;
    unsigned long long total_bytes = 0;
    unsigned long long empty_reads = 0;
    const ULONGLONG started = GetTickCount64();
    while (ok && GetTickCount64() - started < kListenMs &&
           total_messages < kMaxMessages && total_bytes < kMaxBytes) {
        PASSTHRU_MSG messages[kReadBatch] = {};
        unsigned long count = kReadBatch;
        const long status = read(channel, messages, &count, 100);
        if (status == ERR_TIMEOUT || status == ERR_BUFFER_EMPTY) {
            ++empty_reads;
            continue;
        }
        if (status != STATUS_NOERROR) {
            check("PassThruReadMsgs", status, get_last_error);
            ok = false;
            break;
        }
        if (count > kReadBatch) {
            std::printf("Invalid message count returned: %lu (capacity %lu)\n", count, kReadBatch);
            ok = false;
            break;
        }
        for (unsigned long i = 0; i < count; ++i) {
            if (messages[i].DataSize > PASSTHRU_MSG_DATA_SIZE) {
                dump_message(total_messages + 1, messages[i]);
                std::puts("Invalid DataSize; stopping before reading outside the J2534 buffer");
                ok = false;
                break;
            }
            dump_message(total_messages + 1, messages[i]);
            ++total_messages;
            total_bytes += messages[i].DataSize;
            if (total_messages >= kMaxMessages || total_bytes >= kMaxBytes) break;
        }
    }
    std::printf("Capture duration_ms=%llu messages=%llu bytes=%llu empty_reads=%llu limits="
                "%llu_messages/%llu_bytes\n",
                GetTickCount64() - started, total_messages, total_bytes, empty_reads,
                kMaxMessages, kMaxBytes);
    std::puts("Payloads are recorded as opaque J2534 messages; no CAN or ISO-TP boundaries are guessed.");

    if (channel_open) {
        const bool disconnected = check("PassThruDisconnect", disconnect(channel), get_last_error);
        ok = ok && disconnected;
    }
    if (device_open) {
        const bool closed = check("PassThruClose", close(device), get_last_error);
        ok = ok && closed;
    }
    FreeLibrary(dll);
    std::puts("Passive capture ended; no vehicle diagnostic request or transmit API used.");
    return ok ? 0 : 4;
}
