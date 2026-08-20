-- Example ~/.wezterm.lua that shows the CPU temperature from sensors.rs in the
-- status bar, refreshed every 2 seconds.
--
-- Install:
--   mkdir -p ~/.config/wezterm
--   cp cpu_temp.lua ~/.config/wezterm/
--   cp wezterm.lua  ~/.wezterm.lua      # or ~/.config/wezterm/wezterm.lua

local wezterm = require 'wezterm'
local config = wezterm.config_builder()

-- Make sure this directory is on the Lua search path so `require 'cpu_temp'`
-- resolves regardless of where the config itself lives.
package.path = wezterm.config_dir .. '/?.lua;' .. package.path

local cpu_temp = require 'cpu_temp'

cpu_temp.setup {
  -- Sets config.status_update_interval = 2000 for us.
  config = config,
  interval = 2,

  -- The binary is auto-discovered in ~/.local/bin, ~/.cargo/bin,
  -- /opt/homebrew/bin, /usr/local/bin and /usr/bin. GUI apps on macOS do not
  -- inherit your shell PATH, so set this if it lives anywhere else:
  --   command = os.getenv('HOME') .. '/.local/bin/sensors',

  unit = 'C',
  icon = ' ',
  warn_threshold = 65,
  hot_threshold = 80,

  -- Shown instead of the number when no reading is available, so the label
  -- never disappears from the status bar.
  fallback = '-',
  -- Seconds a failed refresh may keep displaying the previous reading.
  stale_after = 30,
}

-- Everything below is just a reasonable looking example config.
config.color_scheme = 'Catppuccin Mocha'
config.use_fancy_tab_bar = false
config.status_update_interval = 2000

return config
