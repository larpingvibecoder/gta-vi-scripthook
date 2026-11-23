// gta-vi-scripthook plugin SDK: header-only C++17 wrapper over scripthook.h.
// SPDX-License-Identifier: MIT OR Apache-2.0
//
//   #include "scripthook.hpp"
//   SHK_PLUGIN_INFO("hello", "0.1.0", "you", "says hello")
//   SHK_EXPORT int SHK_CALL shk_plugin_load(const ShkHost* host, ShkPlugin* self) {
//       shk::init(host, self);
//       shk::info("loaded on %s", host->host_name);
//       shk::on_hotkey(VK_F7, 0, [] { shk::notify("F7"); });
//       shk::start_script("main", [] {
//           for (;;) {
//               int ped = shk::invoke<int>("PLAYER::PLAYER_PED_ID");
//               shk::wait(1000);
//           }
//       });
//       return 0;
//   }
//   SHK_EXPORT void SHK_CALL shk_plugin_unload() { shk::shutdown(); }
//
// RAII: shk::Hook restores its hook, shk::Script stops its script function. Callbacks
// registered through on_frame / on_hotkey / on_draw live until shk::shutdown() (the host removes
// the registrations when the plugin unloads). No exception ever crosses into the host: callbacks
// catch everything and log it.
#ifndef SCRIPTHOOK_HPP
#define SCRIPTHOOK_HPP

#include "scripthook.h"

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <type_traits>
#include <unordered_map>
#include <utility>
#include <vector>

namespace shk {

// ---- the host ---------------------------------------------------------------------------------

namespace detail {
struct State {
    const ShkHost* host = nullptr;
    ShkPlugin* self = nullptr;
    std::mutex mutex;
    std::vector<std::shared_ptr<void>> keep;  // callback storage, freed by shutdown()
    std::unordered_map<std::string, ShkNative> natives;
};
inline State& state() {
    static State s;
    return s;
}
template <class F>
void guarded(F&& f) noexcept {
    try {
        f();
    } catch (const std::exception& e) {
        if (state().host) state().host->core->log(state().self, SHK_LOG_ERROR, e.what());
    } catch (...) {
        if (state().host) state().host->core->log(state().self, SHK_LOG_ERROR, "unknown C++ exception in a callback");
    }
}
template <class T>
std::shared_ptr<T> keep(T value) {
    auto p = std::make_shared<T>(std::move(value));
    std::lock_guard<std::mutex> g(state().mutex);
    state().keep.push_back(p);
    return p;
}
}  // namespace detail

inline void init(const ShkHost* host, ShkPlugin* self) {
    detail::state().host = host;
    detail::state().self = self;
}
inline const ShkHost* host() { return detail::state().host; }
inline ShkPlugin* self() { return detail::state().self; }
// frees callback storage; call from shk_plugin_unload
inline void shutdown() {
    std::lock_guard<std::mutex> g(detail::state().mutex);
    detail::state().keep.clear();
    detail::state().natives.clear();
}

// ---- logging and notices ----------------------------------------------------------------------

inline void vlog(int level, const char* fmt, va_list ap) {
    char buf[1024];
    std::vsnprintf(buf, sizeof buf, fmt, ap);
    host()->core->log(self(), level, buf);
}
inline void log(int level, const char* fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vlog(level, fmt, ap);
    va_end(ap);
}
inline void info(const char* fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vlog(SHK_LOG_INFO, fmt, ap);
    va_end(ap);
}
inline void warn(const char* fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vlog(SHK_LOG_WARN, fmt, ap);
    va_end(ap);
}
inline void notify(const std::string& text, float seconds = 3.0f) { host()->core->notify(self(), text.c_str(), seconds); }
inline std::string plugin_folder() { return host()->core->plugin_folder(self()); }

// ---- memory -------------------------------------------------------------------------------------

template <class T>
std::optional<T> read(ShkAddr addr) {
    static_assert(std::is_trivially_copyable<T>::value, "read<T>: T must be trivially copyable");
    T v;
    if (!host()->memory->read(addr, &v, sizeof v)) return std::nullopt;
    return v;
}
template <class T>
bool write(ShkAddr addr, const T& v) {
    static_assert(std::is_trivially_copyable<T>::value, "write<T>: T must be trivially copyable");
    return host()->memory->write(addr, &v, sizeof v) != 0;
}
inline ShkAddr scan(const char* module, const char* pattern, uint32_t nth = 0, const char* section = nullptr) {
    return host()->memory->scan(module, section, pattern, nth);
}
inline ShkAddr find_export(const char* module, const char* symbol) { return host()->memory->find_export(module, symbol); }

// An inline hook, restored when the object is destroyed.
template <class Fn>
class Hook {
public:
    Hook() = default;
    Hook(const Hook&) = delete;
    Hook& operator=(const Hook&) = delete;
    ~Hook() { remove(); }
    bool install(ShkAddr target, Fn detour) {
        remove();
        void* orig = nullptr;
        handle_ = host()->hooks->install(self(), target, reinterpret_cast<void*>(detour), &orig);
        original_ = reinterpret_cast<Fn>(orig);
        return handle_ != 0;
    }
    void remove() {
        if (handle_) host()->hooks->remove(self(), handle_);
        handle_ = 0;
    }
    Fn original() const { return original_; }
    explicit operator bool() const { return handle_ != 0; }

private:
    ShkHook handle_ = 0;
    Fn original_ = nullptr;
};

// ---- input and frames ---------------------------------------------------------------------------

inline bool on_frame(std::function<void(const ShkFrameInfo&)> fn) {
    auto p = detail::keep(std::move(fn));
    return host()->input->on_frame(
               self(), [](void* u, const ShkFrameInfo* f) { detail::guarded([&] { (*static_cast<std::function<void(const ShkFrameInfo&)>*>(u))(*f); }); }, p.get()) != 0;
}
inline bool on_hotkey(uint32_t vk, uint32_t mods, std::function<void()> fn) {
    auto p = detail::keep(std::move(fn));
    return host()->input->hotkey(self(), vk, mods, [](void* u) { detail::guarded([&] { (*static_cast<std::function<void()>*>(u))(); }); }, p.get()) != 0;
}
inline bool on_pad_chord(uint32_t buttons, std::function<void()> fn) {
    auto p = detail::keep(std::move(fn));
    return host()->input->pad_chord(self(), buttons, [](void* u) { detail::guarded([&] { (*static_cast<std::function<void()>*>(u))(); }); }, p.get()) != 0;
}
inline bool key_down(uint32_t vk) { return host()->input->key_down(vk) != 0; }

// ---- drawing ------------------------------------------------------------------------------------

class Canvas {
public:
    explicit Canvas(const ShkCanvas* c) : c_(c) {}
    uint32_t width() const { return c_->width; }
    uint32_t height() const { return c_->height; }
    float scale() const { return c_->ui_scale; }
    void rect(float x, float y, float w, float h, uint32_t argb) const { c_->rect(c_->ctx, x, y, w, h, argb); }
    float text(float x, float y, const char* s, uint32_t argb, float scale = 0) const { return c_->text(c_->ctx, x, y, scale > 0 ? scale : c_->ui_scale, argb, s); }
    float text_width(const char* s, float scale = 0) const { return c_->measure(c_->ctx, scale > 0 ? scale : c_->ui_scale, s); }
    float line_height(float scale = 0) const { return c_->line_height * (scale > 0 ? scale : c_->ui_scale); }
    void clip(float x, float y, float w, float h) const { c_->clip(c_->ctx, x, y, w, h); }

private:
    const ShkCanvas* c_;
};

// false when the host cannot draw for plugins (host()->draw == NULL)
inline bool on_draw(uint32_t layer, std::function<void(Canvas&)> fn) {
    if (!host()->draw) return false;
    auto p = detail::keep(std::move(fn));
    return host()->draw->on_draw(
               self(), layer,
               [](void* u, const ShkCanvas* c) {
                   detail::guarded([&] {
                       Canvas cv(c);
                       (*static_cast<std::function<void(Canvas&)>*>(u))(cv);
                   });
               },
               p.get()) != 0;
}

// ---- natives --------------------------------------------------------------------------------------

using Vec3 = ShkVec3;
// a 32-bit name hash argument / result
struct Hash {
    uint32_t value = 0;
    Hash() = default;
    explicit Hash(uint32_t v) : value(v) {}
    explicit Hash(const char* text) : value(host()->natives->hash(text)) {}
};
// out parameters: pass the address of an int32_t / float / Vec3, or OutBool for a BOOL*
struct OutBool {
    int32_t* p;
};

namespace detail {
inline ShkValue v(uint32_t t) {
    ShkValue x;
    std::memset(&x, 0, sizeof x);
    x.type = t;
    return x;
}
inline ShkValue arg(int32_t i) { auto x = v(SHK_TYPE_INT); x.u.i = i; return x; }
inline ShkValue arg(uint32_t i) { auto x = v(SHK_TYPE_INT); x.u.i = int32_t(i); return x; }
inline ShkValue arg(bool b) { auto x = v(SHK_TYPE_BOOL); x.u.i = b ? 1 : 0; return x; }
inline ShkValue arg(float f) { auto x = v(SHK_TYPE_FLOAT); x.u.f = f; return x; }
inline ShkValue arg(double f) { return arg(float(f)); }
inline ShkValue arg(Hash h) { auto x = v(SHK_TYPE_HASH); x.u.hash = h.value; return x; }
inline ShkValue arg(const char* s) { auto x = v(SHK_TYPE_STRING); x.u.s = s; return x; }
inline ShkValue arg(const std::string& s) { return arg(s.c_str()); }
inline ShkValue arg(const Vec3& p) { auto x = v(SHK_TYPE_VEC3); x.u.v = p; return x; }
inline ShkValue arg(uint64_t u) { auto x = v(SHK_TYPE_U64); x.u.u64 = u; return x; }
inline ShkValue arg(int32_t* p) { auto x = v(SHK_TYPE_OUT_INT); x.u.p = p; return x; }
inline ShkValue arg(float* p) { auto x = v(SHK_TYPE_OUT_FLOAT); x.u.p = p; return x; }
inline ShkValue arg(OutBool b) { auto x = v(SHK_TYPE_OUT_BOOL); x.u.p = b.p; return x; }
inline ShkValue arg(Vec3* p) { auto x = v(SHK_TYPE_OUT_VEC3); x.u.p = p; return x; }

template <class R> struct Ret;
template <> struct Ret<void> { static constexpr uint32_t type = SHK_TYPE_VOID; };
template <> struct Ret<int> { static constexpr uint32_t type = SHK_TYPE_INT; static int get(const ShkValue& x) { return x.u.i; } };
template <> struct Ret<bool> { static constexpr uint32_t type = SHK_TYPE_BOOL; static bool get(const ShkValue& x) { return x.u.i != 0; } };
template <> struct Ret<float> { static constexpr uint32_t type = SHK_TYPE_FLOAT; static float get(const ShkValue& x) { return x.u.f; } };
template <> struct Ret<Hash> { static constexpr uint32_t type = SHK_TYPE_HASH; static Hash get(const ShkValue& x) { return Hash(x.u.hash); } };
template <> struct Ret<Vec3> { static constexpr uint32_t type = SHK_TYPE_VEC3; static Vec3 get(const ShkValue& x) { return x.u.v; } };
template <> struct Ret<uint64_t> { static constexpr uint32_t type = SHK_TYPE_U64; static uint64_t get(const ShkValue& x) { return x.u.u64; } };
template <> struct Ret<std::string> { static constexpr uint32_t type = SHK_TYPE_STRING; static std::string get(const ShkValue& x) { return x.u.s ? x.u.s : ""; } };
}  // namespace detail

// A native, looked up once.
class Native {
public:
    Native() = default;
    explicit Native(const char* name) { handle_ = host()->natives->lookup(name, &info_); }
    explicit Native(uint64_t hash) {
        char b[32];
        std::snprintf(b, sizeof b, "0x%016llX", (unsigned long long)hash);
        handle_ = host()->natives->lookup(b, &info_);
    }
    bool available() const { return handle_ != 0 && info_.available != 0; }
    const ShkNativeInfo& info() const { return info_; }
    ShkNative handle() const { return handle_; }

    // returns false when the call failed (not in a script, native unknown, bad arguments)
    template <class R, class... A>
    bool call(R* out, A&&... args) const {
        ShkValue a[sizeof...(A) + 1] = {detail::arg(std::forward<A>(args))...};
        ShkValue r = detail::v(detail::Ret<R>::type);
        if (!host()->natives->call(handle_, a, uint32_t(sizeof...(A)), &r)) return false;
        if (out) *out = detail::Ret<R>::get(r);
        return true;
    }
    template <class... A>
    bool call_void(A&&... args) const {
        ShkValue a[sizeof...(A) + 1] = {detail::arg(std::forward<A>(args))...};
        return host()->natives->call(handle_, a, uint32_t(sizeof...(A)), nullptr) != 0;
    }

private:
    ShkNative handle_ = 0;
    ShkNativeInfo info_{};
};

namespace detail {
inline ShkNative cached(const std::string& key) {
    {
        std::lock_guard<std::mutex> g(state().mutex);
        auto it = state().natives.find(key);
        if (it != state().natives.end()) return it->second;
    }
    ShkNative n = host()->natives->lookup(key.c_str(), nullptr);
    std::lock_guard<std::mutex> g(state().mutex);
    state().natives[key] = n;
    return n;
}
inline std::string hash_name(uint64_t h) {
    char b[32];
    std::snprintf(b, sizeof b, "0x%016llX", (unsigned long long)h);
    return b;
}
template <class R, class... A>
R invoke_handle(ShkNative n, A&&... args) {
    ShkValue a[sizeof...(A) + 1] = {arg(std::forward<A>(args))...};
    ShkValue r = v(Ret<R>::type);
    int ok = host()->natives->call(n, a, uint32_t(sizeof...(A)), &r);
    if constexpr (std::is_void<R>::value) {
        (void)ok;
        return;
    } else {
        return ok ? Ret<R>::get(r) : R{};
    }
}
}  // namespace detail

// Calls a native by name ("NS::NAME") or identifier from a script function. Returns R{} when the
// call fails (use shk::Native::call to see failures).
template <class R = void, class... A>
R invoke(const char* name, A&&... args) {
    return detail::invoke_handle<R>(detail::cached(name), std::forward<A>(args)...);
}
template <class R = void, class... A>
R invoke(uint64_t hash, A&&... args) {
    return detail::invoke_handle<R>(detail::cached(detail::hash_name(hash)), std::forward<A>(args)...);
}

inline uint32_t joaat(const char* text) { return host()->natives->hash(text); }

// ---- script functions -------------------------------------------------------------------------

inline bool wait(uint32_t ms = 0) { return host()->scripts && host()->scripts->wait(ms) != 0; }
inline ShkScriptStatus script_status() {
    ShkScriptStatus s{};
    if (host()->scripts) host()->scripts->status(&s);
    return s;
}
inline bool in_script() { return script_status().in_script != 0; }

// A script function on the game's script thread; stopped when the object is destroyed.
class Script {
public:
    Script() = default;
    Script(const char* name, std::function<void()> body) { start(name, std::move(body)); }
    Script(const Script&) = delete;
    Script& operator=(const Script&) = delete;
    ~Script() { stop(); }
    bool start(const char* name, std::function<void()> body) {
        stop();
        if (!host()->scripts) return false;
        body_ = detail::keep(std::move(body));
        id_ = host()->scripts->start(self(), name, [](void* u) { detail::guarded([&] { (*static_cast<std::function<void()>*>(u))(); }); }, body_.get());
        return id_ > 0;
    }
    void stop() {
        if (id_ > 0 && host() && host()->scripts) host()->scripts->stop(self(), id_);
        id_ = 0;
    }
    ShkScript id() const { return id_; }

private:
    ShkScript id_ = 0;
    std::shared_ptr<std::function<void()>> body_;
};

// A script function that runs until it returns or the plugin unloads.
inline ShkScript start_script(const char* name, std::function<void()> body) {
    if (!host()->scripts) return 0;
    auto p = detail::keep(std::move(body));
    return host()->scripts->start(self(), name, [](void* u) { detail::guarded([&] { (*static_cast<std::function<void()>*>(u))(); }); }, p.get());
}

// ---- services ---------------------------------------------------------------------------------

template <class T>
const T* find_service(const char* name) {
    return host()->services ? static_cast<const T*>(host()->services->find(name)) : nullptr;
}
inline const ShkMenuService* menu_service() { return find_service<ShkMenuService>(SHK_MENU_SERVICE); }

}  // namespace shk

#endif  // SCRIPTHOOK_HPP
