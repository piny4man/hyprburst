-- hyprburst.lua — Hyprland 0.55+ (Lua config) drop-in for the hyprburst launcher.
--
-- Since Hyprland 0.55, hyprlang (.conf) is deprecated in favor of Lua, and the
-- two cannot coexist: Hyprland loads ~/.config/hypr/hyprland.lua if present,
-- otherwise ~/.config/hypr/hyprland.conf. If your main config is Lua, use THIS
-- file; if it is still hyprlang, use packaging/hyprburst.conf instead.
--
-- Install it next to hyprland.lua and require it:
--
--     install -Dm644 packaging/hyprburst.lua ~/.config/hypr/hyprburst.lua
--     -- then, in ~/.config/hypr/hyprland.lua:
--     dofile(os.getenv("HOME") .. "/.config/hypr/hyprburst.lua")
--
-- Effect key names follow Hyprland's Lua window-rule API
-- (https://wiki.hypr.land/Configuring/Basics/Window-Rules/). If a future
-- Hyprland build rejects a key, check that page for the version's exact spelling.

-- Super+Space opens the launcher window. hyprburst owns its own Wayland surface,
-- so there is no terminal to resolve — the bare binary opens the window directly.
hl.bind("SUPER + Space", hl.dsp.exec_cmd("hyprburst"))

-- Full-monitor floating overlay + semi-transparent blur, matched by app-id.
-- The app-id is `hyprburst` (configurable via [window] app_id in config.toml —
-- keep these matches in sync if you change it).
hl.window_rule({ match = { class = "hyprburst" }, float = true })
hl.window_rule({ match = { class = "hyprburst" }, size = "monitor_w monitor_h" })
hl.window_rule({ match = { class = "hyprburst" }, move = "0 0" })
hl.window_rule({ match = { class = "hyprburst" }, opacity = "0.9 0.8" })
hl.window_rule({ match = { class = "hyprburst" }, border_size = 0 })
hl.window_rule({ match = { class = "hyprburst" }, no_shadow = true })
hl.window_rule({ match = { class = "hyprburst" }, stay_focused = true })
hl.window_rule({ match = { class = "hyprburst" }, dim_around = true })
