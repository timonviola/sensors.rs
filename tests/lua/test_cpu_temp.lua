-- Test harness for cpu_temp.lua.
--
-- Stubs the `wezterm` module so the plugin can be exercised without WezTerm.
-- Run with:  lua5.4 tests/lua/test_cpu_temp.lua [path-to-sensors-binary]

local here = arg[0]:match '(.*)/[^/]*$' or '.'
package.path = here .. '/../../examples/wezterm/?.lua;' .. package.path

-- ---------------------------------------------------------------- wezterm stub
local child_process = { ok = true, stdout = '' }
local events = {}

package.loaded['wezterm'] = {
  config_dir = '/tmp',
  run_child_process = function(argv)
    child_process.last_argv = argv
    child_process.calls = (child_process.calls or 0) + 1
    -- WezTerm raises a Lua error when the executable does not exist.
    if child_process.missing and not child_process.missing[argv[1]] then
      error('No such file or directory (os error 2)')
    end
    if child_process.raises then
      error('No such file or directory (os error 2)')
    end
    if child_process.spawn then
      -- Really execute the binary under test.
      local cmd = {}
      for _, a in ipairs(argv) do
        cmd[#cmd + 1] = "'" .. a .. "'"
      end
      local pipe = io.popen(table.concat(cmd, ' ') .. ' 2>/dev/null')
      local out = pipe:read 'a'
      local ok = pipe:close()
      return ok and true or false, out, ''
    end
    return child_process.ok, child_process.stdout, ''
  end,
  format = function(items)
    local text, color = '', nil
    for _, item in ipairs(items) do
      if item.Text then
        text = text .. item.Text
      end
      if item.Foreground then
        color = item.Foreground.Color
      end
    end
    return { text = text, color = color }
  end,
  on = function(name, fn)
    events[name] = fn
  end,
}

local cpu_temp = require 'cpu_temp'

-- --------------------------------------------------------------- tiny test lib
local failures, count = 0, 0

local function check(name, got, want)
  count = count + 1
  if got ~= want then
    failures = failures + 1
    print(string.format('FAIL %s\n  got:  %s\n  want: %s', name, tostring(got), tostring(want)))
  else
    print('ok   ' .. name)
  end
end

-- ------------------------------------------------------------------- fixtures
local MACOS_APPLE_SILICON = [[
cpu_thermal-hid-0
Package id 0:  +47.0°C
Core 0:        +45.0°C
Core 1:        +46.5°C
Core 2:        +48.0°C
Core 3:        +49.5°C

applesmc-isa-0300
Fan 1:         1234 RPM  (min = 1200 RPM, max = 5500 RPM)
System total (PSTR):  12.50 W
]]

local LINUX_CORETEMP = [[
coretemp-isa-0000
Package id 0:  +47.0°C  (high = +100.0°C, crit = +100.0°C)
Core 0:        +45.0°C  (high = +100.0°C, crit = +100.0°C)
Core 1:        +49.0°C  (high = +100.0°C, crit = +100.0°C)
]]

-- AMD: no per-core lines at all.
local LINUX_K10TEMP = [[
k10temp-pci-00c3
Tctl:         +52.4°C
Tccd1:        +48.2°C
]]

local NO_CORES_BUT_PACKAGE = [[
coretemp-isa-0000
Package id 0:  +61.0°C
]]

-- ---------------------------------------------------------------------- tests
check('averages Core lines (macOS)', cpu_temp.parse(MACOS_APPLE_SILICON), 47.25)
check('averages Core lines (Linux)', cpu_temp.parse(LINUX_CORETEMP), 47.0)
check('falls back to Package id', cpu_temp.parse(NO_CORES_BUT_PACKAGE), 61.0)
check('falls back to any temperature', cpu_temp.parse(LINUX_K10TEMP), 52.4)
check('ignores fan and power lines', cpu_temp.parse 'Fan 1:  1234 RPM\n', nil)
check('handles empty output', cpu_temp.parse '', nil)
check('handles nil output', cpu_temp.parse(nil), nil)
check('parses negative temperatures', cpu_temp.parse 'Core 0:  -5.0°C\n', -5.0)
check('parses integer temperatures', cpu_temp.parse 'Core 0:  +45°C\n', 45.0)

-- Unit conversion and colours.
cpu_temp.options.unit = 'F'
check('celsius to fahrenheit', cpu_temp.convert(45.0), 113.0)
cpu_temp.options.unit = 'C'
check('celsius passthrough', cpu_temp.convert(45.0), 45.0)

check('normal colour', cpu_temp.color_for(40), cpu_temp.options.colors.normal)
check('warn colour', cpu_temp.color_for(70), cpu_temp.options.colors.warn)
check('hot colour', cpu_temp.color_for(95), cpu_temp.options.colors.hot)

-- Rendering through the stubbed child process.
child_process.ok, child_process.stdout = true, MACOS_APPLE_SILICON
cpu_temp.invalidate()
check('text rendering', cpu_temp.text(), 'CPU 47°C')
check('passes -A to sensors', child_process.last_argv[2], '-A')

cpu_temp.options.unit = 'F'
cpu_temp.invalidate()
check('text rendering in fahrenheit', cpu_temp.text(), 'CPU 117°F')
cpu_temp.options.unit = 'C'

local status = cpu_temp.status()
check('status text', status.text, 'CPU 47°C')
check('status colour', status.color, cpu_temp.options.colors.normal)

-- Caching: repeated calls within the interval must not spawn a process.
cpu_temp.invalidate()
child_process.calls = 0
for _ = 1, 25 do
  cpu_temp.text()
end
check('caches within the interval', child_process.calls, 1)

-- A missing or failing binary keeps the label visible with a placeholder.
child_process.ok, child_process.stdout = false, ''
cpu_temp.reset()
check('failing binary yields placeholder', cpu_temp.text(), 'CPU -')
local unavailable = cpu_temp.status()
check('placeholder is still rendered', unavailable.text, 'CPU -')
check('placeholder uses the unknown colour', unavailable.color, cpu_temp.options.colors.unknown)

-- Regression: a single failed refresh used to blank the label for one tick,
-- which looked like flickering. The last good reading must survive it.
child_process.ok, child_process.stdout = true, MACOS_APPLE_SILICON
cpu_temp.reset()
check('primes the cache', cpu_temp.text(), 'CPU 47°C')
child_process.ok, child_process.stdout = false, ''
for i = 1, 4 do
  cpu_temp.invalidate()
  check('keeps the reading through failure ' .. i, cpu_temp.text(), 'CPU 47°C')
end

-- ... but not forever: once the reading goes stale the placeholder wins.
cpu_temp.options.stale_after = 0
cpu_temp.invalidate()
check('stale reading falls back to placeholder', cpu_temp.text(), 'CPU -')
cpu_temp.options.stale_after = 30

-- Regression: run_child_process *raises* for a missing executable. Before the
-- pcall this took down the whole update-status event.
child_process.ok, child_process.stdout = true, MACOS_APPLE_SILICON
child_process.raises = true
cpu_temp.reset()
local ok_call, result = pcall(cpu_temp.text)
check('missing binary does not raise', ok_call, true)
check('missing binary yields placeholder', result, 'CPU -')
child_process.raises = false

-- Auto-discovery: only one candidate path exists.
local real = os.getenv 'HOME' .. '/.cargo/bin/sensors'
cpu_temp.options.command = nil
cpu_temp.options.candidates = { '/nope/sensors', real, 'sensors' }
child_process.missing = { [real] = true }
cpu_temp.invalidate()
check('discovers a working binary', cpu_temp.text(), 'CPU 47°C')
check('remembers the resolved path', cpu_temp.resolved_command(), real)

-- Once resolved it must not probe the dead paths again.
cpu_temp.invalidate()
child_process.calls = 0
cpu_temp.text()
check('does not re-probe after resolving', child_process.calls, 1)

-- An explicit command overrides discovery.
cpu_temp.options.command = real
check('explicit command wins', cpu_temp.resolved_command(), real)
child_process.missing = nil
cpu_temp.options.command = nil
cpu_temp.options.candidates = { 'sensors' }

-- setup() wires the event and the refresh interval.
local config = {}
cpu_temp.setup { config = config, interval = 2, icon = 'T ' }
check('sets status_update_interval', config.status_update_interval, 2000)
check('registers update-status', type(events['update-status']), 'function')

child_process.ok, child_process.stdout = true, LINUX_CORETEMP
cpu_temp.invalidate()
local shown
events['update-status']({ set_right_status = function(_, v)
  shown = v
end }, nil)
check('event sets right status', shown.text, 'T 47°C')

-- Optionally drive the real binary end to end.
local binary = arg[1]
if binary then
  child_process.spawn = true
  cpu_temp.options.command = binary
  cpu_temp.options.icon = 'CPU '
  cpu_temp.invalidate()
  local out = cpu_temp.text()
  check('real binary produces a reading', out:match '^CPU %d+°C$' ~= nil, true)
  print('     -> ' .. out)
end

print(string.format('\n%d checks, %d failures', count, failures))
os.exit(failures == 0 and 0 or 1)
