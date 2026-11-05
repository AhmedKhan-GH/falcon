// falcon-ifc-worker: runs web-ifc in a process of its own, so that a crash, a
// hang or a runaway allocation on a malformed file ends only this process.
//
// stdin holds the request: circle segments (u16), timeout in milliseconds
// (u64) and the memory limit in bytes (u64), little-endian, then the IFC file
// up to end of file. stdout carries the reply as it is read, as Online 3D
// Viewer reads a file (LoadAllGeometry with COORDINATE_TO_ORIGIN, openings
// and spaces left out): the magic "FIFC", then records:
//
//   'G' one placed geometry: product (u32), colour (4 f64), column-major
//       placement (16 f64), vertex float count (u32), index count (u32), the
//       vertices (six f32 each: position, normal), the indices (u32)
//   'P' one product looked at, after its geometries: id (u32), length of the
//       entity name (u32), the name
//   'E' the end of a good read
//   'O' web-ifc could not open the file
//   'F' web-ifc failed: length of the message (u32), the message
//
// The exit status is 0 after one of the last three, otherwise one of the EXIT
// values below (or death by signal). Windows has no signals, so an unhandled
// exception ends the process with EXIT_CRASH there, without a crash dialog.
//
// This file is falcon's own (MIT OR Apache-2.0); it only calls web-ifc's
// public classes.

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <functional>
#include <string>
#include <vector>

#include <fcntl.h>
#include <signal.h>
#ifndef _WIN32
#include <pthread.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>
#endif

#include <spdlog/spdlog.h>

#include "web-ifc/modelmanager/ModelManager.h"
#include "web-ifc/schema/IfcSchemaManager.h"
#include "web-ifc/schema/ifc-schema.h"

// After web-ifc's headers, whose names windows.h's macros would otherwise clash with.
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <crtdbg.h>
#include <io.h>
#include <process.h>
#include <psapi.h>
#define THREAD unsigned __stdcall
#else
#define THREAD void *
#endif

namespace {

constexpr int EXIT_BAD_REQUEST = 69;
constexpr int EXIT_CRASH = 70;
constexpr int EXIT_TIMEOUT = 71;
constexpr int EXIT_MEMORY = 72;

// web-ifc's recursion over deep instance graphs needs far more than a default
// thread's stack.
constexpr size_t READER_STACK = size_t(256) << 20;
// What's left to handle a stack overflow on: the signal stack, or on Windows the guarantee.
constexpr size_t SIGNAL_STACK = size_t(64) << 10;

struct Request {
    uint16_t circle_segments = 0;
    uint64_t timeout_ms = 0;
    uint64_t max_memory = 0;
    std::vector<uint8_t> file;
};

Request request;
FILE *reply = nullptr;

void put(const void *data, size_t size) {
    if (size > 0 && fwrite(data, 1, size, reply) != size) {
        _exit(EXIT_CRASH);
    }
}

void put_u32(uint32_t value) { put(&value, sizeof value); }

void put_tag(char tag) { put(&tag, 1); }

#ifdef _WIN32
// TerminateProcess needs almost no stack and no locks, whatever state the crash left.
[[noreturn]] void crash() {
    TerminateProcess(GetCurrentProcess(), EXIT_CRASH);
    _exit(EXIT_CRASH);
}

LONG WINAPI on_fatal_exception(EXCEPTION_POINTERS *) { crash(); }

void on_abort(int) { crash(); }

void on_invalid_parameter(const wchar_t *, const wchar_t *, const wchar_t *, unsigned, uintptr_t) { crash(); }

void on_pure_call() { crash(); }

// Ends quietly on anything fatal, so a bad file brings up no dialog or crash report.
void catch_crashes() {
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    SetUnhandledExceptionFilter(on_fatal_exception);
    _set_abort_behavior(0, _WRITE_ABORT_MSG | _CALL_REPORTFAULT);
    signal(SIGABRT, on_abort);
    _set_invalid_parameter_handler(on_invalid_parameter);
    _set_purecall_handler(on_pure_call);
}

// Leaves room for the exception filter to run after a stack overflow.
void prepare_stack() {
    ULONG guarantee = SIGNAL_STACK;
    SetThreadStackGuarantee(&guarantee);
}

uint64_t now_ms() { return GetTickCount64(); }

uint64_t peak_memory() {
    PROCESS_MEMORY_COUNTERS counters;
    std::memset(&counters, 0, sizeof counters);
    GetProcessMemoryInfo(GetCurrentProcess(), &counters, sizeof counters);
    return uint64_t(counters.PeakWorkingSetSize);
}

void sleep_50ms() { Sleep(50); }

// Starts a thread that reserves `stack` bytes of address space, or the default for 0. Never fails silently.
bool start_thread(THREAD fn(void *), size_t stack, HANDLE *out = nullptr) {
    uintptr_t handle = _beginthreadex(nullptr, unsigned(stack), fn, nullptr, stack ? STACK_SIZE_PARAM_IS_A_RESERVATION : 0, nullptr);
    if (out) {
        *out = reinterpret_cast<HANDLE>(handle);
    }
    return handle != 0;
}
#else
void on_fatal_signal(int) { _exit(EXIT_CRASH); }

// Ends quietly on a fatal signal, so a bad file leaves no crash report.
void catch_crashes() {
    struct sigaction action;
    std::memset(&action, 0, sizeof action);
    action.sa_handler = on_fatal_signal;
    action.sa_flags = SA_ONSTACK;
    sigemptyset(&action.sa_mask);
    for (int signal : {SIGSEGV, SIGBUS, SIGABRT, SIGILL, SIGFPE, SIGTRAP}) {
        sigaction(signal, &action, nullptr);
    }
}

// The signal stack is what lets the handler run after a stack overflow.
void prepare_stack() {
    stack_t alt;
    alt.ss_sp = std::malloc(SIGNAL_STACK);
    alt.ss_size = SIGNAL_STACK;
    alt.ss_flags = 0;
    sigaltstack(&alt, nullptr);
}

uint64_t now_ms() {
    timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return uint64_t(t.tv_sec) * 1000 + uint64_t(t.tv_nsec) / 1000000;
}

uint64_t peak_memory() {
    rusage usage;
    std::memset(&usage, 0, sizeof usage);
    getrusage(RUSAGE_SELF, &usage);
#ifdef __APPLE__
    return uint64_t(usage.ru_maxrss);
#else
    return uint64_t(usage.ru_maxrss) * 1024;
#endif
}

void sleep_50ms() { usleep(50000); }
#endif

// Ends the process past its limits, also when the parent has gone.
THREAD watch(void *) {
    uint64_t start = now_ms();
    for (;;) {
        sleep_50ms();
        if (now_ms() - start > request.timeout_ms) {
            _exit(EXIT_TIMEOUT);
        }
        if (peak_memory() > request.max_memory) {
            _exit(EXIT_MEMORY);
        }
    }
}

// Writes every product's geometry. Returns false when the file can't be opened.
bool read_model() {
    spdlog::set_level(spdlog::level::off);
    webifc::manager::ModelManager manager(false);
    manager.SetLogLevel(spdlog::level::off);
    webifc::manager::LoaderSettings settings;
    settings.COORDINATE_TO_ORIGIN = true;
    if (request.circle_segments > 0) {
        settings.CIRCLE_SEGMENTS = request.circle_segments;
    }
    uint32_t model = manager.CreateModel(settings);
    auto loader = manager.GetIfcLoader(model);
    const uint8_t *data = request.file.data();
    size_t len = request.file.size();
    const std::function<uint32_t(char *, size_t, size_t)> feed = [&](char *dest, size_t offset, size_t size) -> uint32_t {
        if (offset >= len) {
            return 0;
        }
        size_t n = std::min(size, len - offset);
        std::memcpy(dest, data + offset, n);
        return uint32_t(n);
    };
    if (!loader->LoadFile(feed)) {
        return false;
    }
    auto processor = manager.GetGeometryProcessor(model);
    const auto &schema = manager.GetSchemaManager();
    std::vector<uint32_t> types(schema.GetIfcElementList().begin(), schema.GetIfcElementList().end());
    std::sort(types.begin(), types.end());
    for (uint32_t type : types) {
        if (type == webifc::schema::IFCOPENINGELEMENT || type == webifc::schema::IFCSPACE ||
            type == webifc::schema::IFCOPENINGSTANDARDCASE) {
            continue;
        }
        std::string name;
        for (uint32_t id : loader->GetExpressIDsWithType(type)) {
            if (name.empty()) {
                name = schema.IfcTypeCodeToType(type);
            }
            // One product that web-ifc cannot process is left out; the rest are read.
            try {
                auto mesh = processor->GetFlatMesh(id, true);
                for (auto &placed : mesh.geometries) {
                    auto &geometry = processor->GetGeometry(placed.geometryExpressID);
                    auto vertices = reinterpret_cast<const float *>(geometry.GetVertexData());
                    uint32_t vertex_floats = geometry.GetVertexDataSize();
                    // The index pointer is only taken from a non-empty list.
                    uint32_t index_count = geometry.GetIndexDataSize();
                    if (vertex_floats < 6 || index_count < 3 || !vertices) {
                        continue;
                    }
                    auto indices = reinterpret_cast<const uint32_t *>(geometry.GetIndexData());
                    double color[4] = {placed.color.r, placed.color.g, placed.color.b, placed.color.a};
                    put_tag('G');
                    put_u32(id);
                    put(color, sizeof color);
                    put(placed.flatTransformation.data(), 16 * sizeof(double));
                    put_u32(vertex_floats);
                    put_u32(index_count);
                    put(vertices, vertex_floats * sizeof(float));
                    put(indices, index_count * sizeof(uint32_t));
                }
            } catch (...) {
            }
            put_tag('P');
            put_u32(id);
            put_u32(uint32_t(name.size()));
            put(name.data(), name.size());
            processor->Clear(true);
        }
    }
    manager.CloseModel(model);
    return true;
}

void put_failure(const char *message) {
    put_tag('F');
    put_u32(uint32_t(std::strlen(message)));
    put(message, std::strlen(message));
}

THREAD read_and_reply(void *) {
    prepare_stack();
    try {
        put_tag(read_model() ? 'E' : 'O');
    } catch (const std::exception &e) {
        put_failure(e.what());
    } catch (...) {
        put_failure("unknown error");
    }
    if (fflush(reply) != 0) {
        _exit(EXIT_CRASH);
    }
    _exit(0);
    return 0;
}

bool read_all(int fd, std::vector<uint8_t> &out) {
    std::vector<uint8_t> chunk(1 << 20);
    for (;;) {
        auto n = read(fd, chunk.data(), static_cast<unsigned>(chunk.size()));
        if (n < 0) {
            return false;
        }
        if (n == 0) {
            return true;
        }
        out.insert(out.end(), chunk.begin(), chunk.begin() + n);
    }
}

} // namespace

int main() {
    // The reply gets its own descriptor; anything web-ifc prints goes nowhere.
    int out = dup(1);
#ifdef _WIN32
    int null = open("NUL", O_WRONLY);
    // The request and the reply are binary, and Windows's C runtime would translate line ends.
    _setmode(0, _O_BINARY);
    _setmode(out, _O_BINARY);
#else
    int null = open("/dev/null", O_WRONLY);
#endif
    if (out < 0 || null < 0 || dup2(null, 1) < 0 || !(reply = fdopen(out, "wb"))) {
        return EXIT_BAD_REQUEST;
    }
    setvbuf(reply, nullptr, _IOFBF, 1 << 20);
    std::vector<uint8_t> input;
    constexpr size_t HEADER = 2 + 8 + 8;
    if (!read_all(0, input) || input.size() < HEADER) {
        return EXIT_BAD_REQUEST;
    }
    std::memcpy(&request.circle_segments, input.data(), 2);
    std::memcpy(&request.timeout_ms, input.data() + 2, 8);
    std::memcpy(&request.max_memory, input.data() + 10, 8);
    input.erase(input.begin(), input.begin() + HEADER);
    request.file = std::move(input);
    put("FIFC", 4);
    catch_crashes();
#ifdef _WIN32
    HANDLE reader;
    if (!start_thread(watch, 0) || !start_thread(read_and_reply, READER_STACK, &reader)) {
        return EXIT_CRASH;
    }
    WaitForSingleObject(reader, INFINITE);
    return EXIT_CRASH;
#else
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_t watcher, reader;
    if (pthread_create(&watcher, &attr, watch, nullptr) != 0) {
        return EXIT_CRASH;
    }
    pthread_attr_setstacksize(&attr, READER_STACK);
    if (pthread_create(&reader, &attr, read_and_reply, nullptr) != 0) {
        return EXIT_CRASH;
    }
    pthread_join(reader, nullptr);
    return EXIT_CRASH;
#endif
}
