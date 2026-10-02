// Adapter-only diagnostic probe. ABI: Tactrix's installed j2534_tactrix.h.
// Build as x86. Identity is the default; --opta-passive explicitly opens a sniff channel.
// No transmit API is loaded in either mode.
#include <windows.h>
#include <cstdio>
#include <cstring>
#include "C:/Program Files (x86)/OpenECU/OpenPort 2.0/samples/common/j2534_tactrix.h"

int main(int argc, char** argv) {
    static_assert(sizeof(void*) == 4, "The installed DLL requires an x86 process");
    bool passive = argc == 2 && std::strcmp(argv[1], "--opta-passive") == 0;
    bool opta = passive || (argc == 2 && std::strcmp(argv[1], "--opta") == 0);
    if (argc != 1 && !opta) {
        std::puts("Usage: openport2-identity.exe [--opta | --opta-passive]");
        return 1;
    }
    const wchar_t* path = opta
        ? L"C:\\Program Files (x86)\\OPTA\\Tactrix\\opta_j2534_0404_tactrix.dll"
        : L"C:\\Windows\\SysWOW64\\op20pt32.dll";
    HMODULE dll = LoadLibraryExW(path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!dll) {
        std::printf("LoadLibraryExW failed: %lu\n", GetLastError());
        return 1;
    }
    auto ioctl = reinterpret_cast<PF_PassThruIoctl*>(GetProcAddress(dll, "PassThruIoctl"));
    auto open = reinterpret_cast<PF_PassThruOpen*>(GetProcAddress(dll, "PassThruOpen"));
    auto version = reinterpret_cast<PF_PassThruReadVersion*>(GetProcAddress(dll, "PassThruReadVersion"));
    auto close = reinterpret_cast<PF_PassThruClose*>(GetProcAddress(dll, "PassThruClose"));
    auto error = reinterpret_cast<PF_PassThruGetLastError*>(GetProcAddress(dll, "PassThruGetLastError"));
    if (!ioctl || !open || !version || !close || !error) {
        std::puts("Required adapter-identification export missing");
        FreeLibrary(dll);
        return 1;
    }
    auto check = [&](const char* name, long status) {
        std::printf("%s: 0x%08lX\n", name, status);
        std::fflush(stdout);
        if (status) {
            char detail[512] = {};
            long error_status = error(detail);
            std::printf("PassThruGetLastError: 0x%08lX, %.*s\n",
                error_status, 511, detail);
        }
        return status == STATUS_NOERROR;
    };
    std::puts(opta
        ? "DLL: OPTA opta_j2534_0404_tactrix.dll; process: x86"
        : "DLL: C:\\Windows\\SysWOW64\\op20pt32.dll; process: x86");
    if (!opta) {
        wchar_t instances[512] = {};
        if (!check("TX_IOCTL_GET_DEVICE_INSTANCES",
                ioctl(0, TX_IOCTL_GET_DEVICE_INSTANCES, nullptr, instances))) {
            FreeLibrary(dll);
            return 2;
        }
        instances[511] = 0;
        char utf8[2048] = {};
        WideCharToMultiByte(CP_UTF8, 0, instances, -1, utf8, sizeof(utf8), nullptr, nullptr);
        std::printf("Device instance: %s\n", utf8);
    }
    unsigned long device = 0;
    if (!check("PassThruOpen", open(nullptr, &device))) {
        FreeLibrary(dll);
        return 3;
    }
    char firmware[512] = {}, dll_version[512] = {}, api[512] = {};
    bool ok = check("PassThruReadVersion", version(device, firmware, dll_version, api));
    if (ok) {
        std::printf("Firmware: %.*s\nDLL: %.*s\nAPI: %.*s\n",
            511, firmware, 511, dll_version, 511, api);
    }
    if (ok && passive) {
        if (std::strcmp(firmware, "main code version : 1.17.4877") != 0 ||
                std::strcmp(api, "04.04") != 0) {
            std::puts("Unexpected adapter version: stopping before channel open");
            ok = false;
        } else {
            static_assert(sizeof(PASSTHRU_MSG) == 4152, "Unexpected J2534 ABI layout");
            auto connect = reinterpret_cast<PF_PassThruConnect*>(GetProcAddress(dll, "PassThruConnect"));
            auto disconnect = reinterpret_cast<PF_PassThruDisconnect*>(GetProcAddress(dll, "PassThruDisconnect"));
            auto filter = reinterpret_cast<PF_PassThruStartMsgFilter*>(GetProcAddress(dll, "PassThruStartMsgFilter"));
            auto read = reinterpret_cast<PF_PassThruReadMsgs*>(GetProcAddress(dll, "PassThruReadMsgs"));
            if (!connect || !disconnect || !filter || !read) {
                std::puts("Passive channel export missing");
                ok = false;
            } else {
                unsigned long channel = 0;
                std::puts("CAN5; flags=0x10000800 (SNIFF_MODE | CAN_ID_BOTH); 500000 bit/s");
                ok = check("PassThruConnect", connect(device, CAN,
                    SNIFF_MODE | CAN_ID_BOTH, 500000, &channel));
                if (ok) {
                    PASSTHRU_MSG mask = {}, pattern = {};
                    mask.ProtocolID = pattern.ProtocolID = CAN;
                    mask.DataSize = pattern.DataSize = 4;
                    unsigned long filter_id = 0;
                    ok = check("PassThruStartMsgFilter (PASS; TxFlags=0; no flow control)",
                        filter(channel, PASS_FILTER, &mask, &pattern, nullptr, &filter_id));
                    unsigned long received = 0, timeouts = 0;
                    ULONGLONG started = GetTickCount64();
                    while (ok && GetTickCount64() - started < 5000) {
                        PASSTHRU_MSG message = {};
                        unsigned long count = 1;
                        long status = read(channel, &message, &count, 100);
                        if (status == ERR_TIMEOUT || status == ERR_BUFFER_EMPTY) {
                            ++timeouts;
                            continue;
                        }
                        if (status != STATUS_NOERROR) {
                            check("PassThruReadMsgs", status);
                            ok = false;
                            break;
                        }
                        if (!count) continue;
                        if (count != 1 || message.ProtocolID != CAN ||
                                message.DataSize < 4 || message.DataSize > 12 ||
                                (message.RxStatus & (TX_MSG_TYPE | TX_DONE))) {
                            std::printf("Unexpected frame: count=%lu ProtocolID=%lu "
                                "DataSize=%lu ExtraDataIndex=%lu RxStatus=0x%08lX "
                                "TxFlags=0x%08lX Timestamp=%lu bytes=",
                                count, message.ProtocolID, message.DataSize,
                                message.ExtraDataIndex, message.RxStatus,
                                message.TxFlags, message.Timestamp);
                            unsigned long prefix = message.DataSize;
                            if (prefix > 32) prefix = 32;
                            if (prefix > sizeof(message.Data)) prefix = sizeof(message.Data);
                            for (unsigned long i = 0; i < prefix; ++i)
                                std::printf("%02X ", message.Data[i]);
                            if (message.DataSize > prefix) std::printf("...");
                            std::puts("; stopping");
                            ok = false;
                            break;
                        }
                        ++received;
                        if (received <= 12) {
                            std::printf("RX #%lu timestamp_us=%lu status=0x%lX bytes=",
                                received, message.Timestamp, message.RxStatus);
                            for (unsigned long i = 0; i < message.DataSize; ++i)
                                std::printf("%02X ", message.Data[i]);
                            std::puts("");
                        }
                    }
                    std::printf("Listen duration_ms=%llu; raw messages=%lu; empty/timeouts=%lu\n",
                        GetTickCount64() - started, received, timeouts);
                    bool disconnected = check("PassThruDisconnect", disconnect(channel));
                    ok = ok && disconnected;
                }
            }
        }
    }
    bool closed = check("PassThruClose", close(device));
    FreeLibrary(dll);
    std::puts(passive ? "Passive test ended; no transmit API loaded."
        : "No vehicle channel opened; no vehicle message sent.");
    return ok && closed ? 0 : 4;
}
