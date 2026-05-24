/*
 * GriddyWM Plugin ABI — version 1
 *
 * Compile your plugin as a shared library:
 *   cc -shared -fPIC -o libmyplugin.so myplugin.c -I/usr/include/griddy
 *
 * Then add to ~/.config/griddy/plugins.toml:
 *   [[plugin]]
 *   path = "/path/to/libmyplugin.so"
 *   enabled = true
 *
 * Your plugin must export exactly one function:
 *   GriddyPluginDescriptor* griddy_plugin_init(void);
 *
 * The returned descriptor must remain valid for the lifetime of the plugin
 * (declare it as a static variable).
 *
 * ABI stability: fields may be added to the end of structs in future versions.
 * Always check abi_version before accessing new fields.
 */

#pragma once

#include <stdint.h>

#define GRIDDY_PLUGIN_ABI_VERSION 1

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque compositor handle ─────────────────────────────────────────────── */

/**
 * Passed to every hook.  Plugins must not dereference or cast this pointer.
 * It is reserved for future compositor API calls (e.g. registering custom
 * dispatchers or querying window state).
 */
typedef struct GriddyPluginHandle GriddyPluginHandle;

/* ── Window info passed to lifecycle hooks ────────────────────────────────── */

/**
 * Per-window metadata passed to on_window_pre_open / on_window_post_close.
 *
 * LIFETIME: The GriddyWindowInfo pointer and the strings it contains (app_id,
 * title) are ONLY valid for the duration of the hook call.  Do NOT store the
 * pointer or copy the raw char* pointers across hook boundaries; copy the
 * string contents if you need them later (e.g. with strdup).
 */
typedef struct GriddyWindowInfo {
    uint32_t    id;       /**< Compositor-internal window ID.                */
    const char *app_id;   /**< Wayland app_id / WM_CLASS (may be NULL).      */
    const char *title;    /**< Window title (may be NULL).                   */
    int32_t     x;        /**< Logical screen X of the window's slot.        */
    int32_t     y;        /**< Logical screen Y.                             */
    int32_t     w;        /**< Width in logical pixels.                      */
    int32_t     h;        /**< Height in logical pixels.                     */
} GriddyWindowInfo;

/* ── Plugin descriptor ────────────────────────────────────────────────────── */

typedef struct GriddyPluginDescriptor {
    /** Short identifier, e.g. "touchpad-tweaks". Must not be NULL. */
    const char *name;

    /**
     * Must equal GRIDDY_PLUGIN_ABI_VERSION (1).
     * The compositor refuses to load plugins with a mismatched version and
     * emits a "plugin_error" event on the IPC event socket.
     */
    uint32_t abi_version;

    /** Plugin-private pointer; never read or freed by the compositor. */
    void *user_data;

    /**
     * Called once after ABI verification passes.
     * `config_toml` is the raw TOML content of the plugin's config block
     * from plugins.toml (may be an empty string, never NULL).
     * Return 0 on success; non-zero aborts loading.
     */
    int (*on_load)(GriddyPluginHandle *handle, const char *config_toml);

    /** Called once just before the shared library is unloaded. */
    void (*on_unload)(GriddyPluginHandle *handle);

    /** Called at the beginning of every compositor frame, before rendering. */
    void (*on_frame_begin)(GriddyPluginHandle *handle);

    /** Called at the end of every compositor frame, after submit. */
    void (*on_frame_end)(GriddyPluginHandle *handle);

    /** Called just before the GLES frame is opened (before clear + draw). */
    void (*on_pre_render)(GriddyPluginHandle *handle);

    /** Called just after the GLES frame is finished (before buffer swap). */
    void (*on_post_render)(GriddyPluginHandle *handle);

    /**
     * Called when a new toplevel window is about to be placed in the grid.
     * `info->id` is 0 at this stage (window not yet assigned).
     */
    void (*on_window_pre_open)(GriddyPluginHandle *handle, const GriddyWindowInfo *info);

    /** Called after a window has been fully removed from the grid. */
    void (*on_window_post_close)(GriddyPluginHandle *handle, const GriddyWindowInfo *info);
} GriddyPluginDescriptor;

/* ── Required export ─────────────────────────────────────────────────────── */

/**
 * Every plugin must export this function.  Return a pointer to a static
 * GriddyPluginDescriptor; the compositor does NOT free it.
 * All function-pointer fields are optional — set unused hooks to NULL.
 */
GriddyPluginDescriptor *griddy_plugin_init(void);

/* ── Minimal example ─────────────────────────────────────────────────────── */
/*
 * #include "griddy_plugin.h"
 * #include <stdio.h>
 *
 * static void on_frame_begin(GriddyPluginHandle *h) {
 *     (void)h;
 *     // called every frame
 * }
 *
 * static GriddyPluginDescriptor DESC = {
 *     .name        = "hello-griddy",
 *     .abi_version = GRIDDY_PLUGIN_ABI_VERSION,
 *     .on_frame_begin = on_frame_begin,
 * };
 *
 * GriddyPluginDescriptor *griddy_plugin_init(void) { return &DESC; }
 */

#ifdef __cplusplus
} /* extern "C" */
#endif
