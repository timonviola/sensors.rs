-- cpu_temp.lua - show the CPU temperature from sensors.rs in the WezTerm
-- status bar.
--
-- Works on macOS (sensors.rs) and Linux (sensors.rs or lm-sensors), because it
-- parses the standard `sensors` output format.
--
-- Usage in ~/.wezterm.lua:
--
--   local cpu_temp = require 'cpu_temp'
--   cpu_temp.setup { interval = 2 }
--
-- See wezterm.lua in this directory for a complete example.

local wezterm = require 'wezterm'

local M = {}

M.options = {
  -- Path to the sensors binary. When nil, the paths in M.options.candidates
  -- are probed and the first working one is remembered. GUI apps on macOS do
  -- not inherit the shell PATH, hence the absolute paths.
  command = nil,
  candidates = {
    (os.getenv 'HOME' or '') .. '/.local/bin/sensors',
    (os.getenv 'HOME' or '') .. '/.cargo/bin/sensors',
    '/opt/homebrew/bin/sensors',
    '/usr/local/bin/sensors',
    '/usr/bin/sensors',
    'sensors',
  },
  -- Refresh interval in seconds.
  interval = 2,
  -- 'C' or 'F'.
  unit = 'C',
  -- Prefix shown before the reading.
  icon = 'CPU ',
  -- Reading is coloured by these thresholds, in degrees Celsius.
  warn_threshold = 65,
  hot_threshold = 80,
  colors = {
    normal = '#a6e3a1',
    warn = '#f9e2af',
    hot = '#f38ba8',
    -- Used for the fallback placeholder below.
    unknown = '#6c7086',
  },
  -- Placeholder shown in place of the reading when none is available. The
  -- icon is always kept, so the status item never disappears and the rest of
  -- the status bar does not jump around.
  fallback = '-',
  -- How long (seconds) the last good reading keeps being displayed when a
  -- refresh fails. This absorbs transient hiccups - a single failed spawn used
  -- to blank the label for one tick, which looked like flickering. Set to 0 to
  -- switch to the placeholder as soon as one read fails.
  stale_after = 30,
}

--- Extracts a temperature in degrees Celsius from `sensors` output.
--
-- Averages every `Core N:` line, which is what the tmux-cpu plugin does and
-- what sensors.rs emits on macOS. Falls back to `Package id N:` and finally to
-- any temperature line, so it also works with lm-sensors chips that expose no
-- per-core readings (k10temp, thinkpad, ...).
--
-- @param output string  raw stdout of `sensors`
-- @return number|nil    degrees Celsius
function M.parse(output)
  if not output or output == '' then
    return nil
  end

  local core_sum, core_count = 0, 0
  local package_temp, any_temp = nil, nil

  for line in output:gmatch '[^\r\n]+' do
    -- Matches "Core 0:        +45.0°C  (high = ...)"; the sign and the degree
    -- symbol are optional so lm-sensors and sensors.rs both parse.
    local value = line:match '^Core%s+%d+:%s*([%+%-]?%d+%.?%d*)'
    if value then
      core_sum = core_sum + tonumber(value)
      core_count = core_count + 1
    else
      local pkg = line:match '^Package id%s+%d+:%s*([%+%-]?%d+%.?%d*)'
      if pkg then
        package_temp = tonumber(pkg)
      elseif not any_temp then
        -- Any label followed by a temperature, e.g. "Tctl:  +52.4°C".
        local other = line:match '^[^:]+:%s*([%+%-]?%d+%.?%d*)%s*°?[CF]'
        if other then
          any_temp = tonumber(other)
        end
      end
    end
  end

  if core_count > 0 then
    return core_sum / core_count
  end
  return package_temp or any_temp
end

--- Converts Celsius to the configured unit.
function M.convert(celsius)
  if M.options.unit == 'F' then
    return celsius * 9 / 5 + 32
  end
  return celsius
end

--- Picks a colour for a reading (always compared in Celsius).
function M.color_for(celsius)
  local o = M.options
  if celsius >= o.hot_threshold then
    return o.colors.hot
  elseif celsius >= o.warn_threshold then
    return o.colors.warn
  end
  return o.colors.normal
end

-- Cached reading, so a short status_update_interval cannot spawn a process on
-- every repaint. `at` is the last attempt, `ok_at` the last success; keeping
-- both lets a failed refresh reuse the previous value instead of blanking the
-- status item.
local cache = { celsius = nil, at = -math.huge, ok_at = -math.huge }

-- The binary path that last worked, so we probe the candidates only once.
local resolved = nil

--- Runs one sensors binary.
--
-- wezterm.run_child_process raises a Lua error when the executable does not
-- exist, so the call must be wrapped in pcall; otherwise a wrong path takes
-- down the whole update-status event.
--
-- @return string|nil stdout
local function run(command)
  local called, ok, stdout = pcall(wezterm.run_child_process, { command, '-A' })
  if not called or not ok then
    return nil
  end
  return stdout
end

--- The cached reading, dropped when it is older than `stale_after`.
-- Called only after a failed refresh, so a successful reading is always
-- served for the whole interval.
-- @param now number  current epoch seconds
-- @return number|nil degrees Celsius
local function keep_or_drop(now)
  local stale_after = M.options.stale_after or 0
  if stale_after <= 0 or (now - cache.ok_at) > stale_after then
    cache.celsius = nil
  end
  return cache.celsius
end

--- Runs `sensors` at most once per `interval` seconds.
--
-- A failed run does not clear the last good reading; it is kept for
-- `stale_after` seconds so a transient failure cannot make the status item
-- blink out.
--
-- @return number|nil degrees Celsius
function M.read()
  local now = os.time()
  if now - cache.at < M.options.interval then
    return cache.celsius
  end
  cache.at = now

  -- An explicitly configured command is used as-is; otherwise probe.
  local candidates = M.options.command and { M.options.command }
      or (resolved and { resolved } or M.options.candidates)

  for _, command in ipairs(candidates) do
    local stdout = run(command)
    if stdout then
      local celsius = M.parse(stdout)
      if celsius then
        resolved = command
        cache.celsius = celsius
        cache.ok_at = now
        return celsius
      end
    end
  end

  -- Nothing worked; probe the full list again on the next read and keep
  -- showing the previous reading until it goes stale.
  resolved = nil
  return keep_or_drop(now)
end

--- The sensors binary currently in use, or nil if none was found.
function M.resolved_command()
  return M.options.command or resolved
end

--- Forces the next read() to spawn a process again.
-- The last good reading is kept, so a failing refresh still renders it.
function M.invalidate()
  cache.at = -math.huge
end

--- Drops the cached reading entirely, so the next failure renders the
-- fallback placeholder immediately.
function M.reset()
  cache.celsius = nil
  cache.at = -math.huge
  cache.ok_at = -math.huge
end

--- Plain text for the current reading, e.g. "CPU 45°C".
-- Never empty: without a reading the icon is kept and the value is replaced by
-- `options.fallback` ("CPU -"), so the status item cannot disappear.
function M.text(celsius)
  if celsius == nil then
    celsius = M.read()
  end
  if not celsius then
    return M.options.icon .. M.options.fallback
  end
  return string.format('%s%.0f°%s', M.options.icon, M.convert(celsius), M.options.unit)
end

--- Formatted (coloured) status bar item.
function M.status()
  local celsius = M.read()
  local color = celsius and M.color_for(celsius) or M.options.colors.unknown
  return wezterm.format {
    { Foreground = { Color = color } },
    { Text = M.text(celsius) },
  }
end

--- Wires the reading into the right hand side of the status bar.
--
-- @param opts table|nil overrides for M.options, plus an optional
--                      `config` table to set status_update_interval on.
function M.setup(opts)
  opts = opts or {}
  local config = opts.config
  for key, value in pairs(opts) do
    if key ~= 'config' then
      M.options[key] = value
    end
  end

  -- WezTerm fires update-status on this interval (milliseconds).
  if config then
    config.status_update_interval = M.options.interval * 1000
  end

  wezterm.on('update-status', function(window, _pane)
    window:set_right_status(M.status())
  end)

  return M
end

return M
