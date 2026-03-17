#!/usr/bin/osascript
-- Launches `p gui` in a new terminal window.
-- Tries iTerm2 first, falls back to Terminal.app.
--
-- To bind to a hotkey:
--   1. Open Automator → New → Quick Action
--   2. Set "Workflow receives" to "no input" in "any application"
--   3. Add "Run AppleScript" action, paste this script
--   4. Save (e.g. "Project Switcher")
--   5. System Settings → Keyboard → Shortcuts → Services
--      → find "Project Switcher" → assign your hotkey (e.g. ⌃⌘P)

on run
  set shellInit to "export PATH=\"$HOME/.bun/bin:$PATH\"; "

  if applicationIsRunning("iTerm") or not applicationIsRunning("Terminal") then
    try
      tell application "iTerm"
        activate
        create window with default profile command shellInit & "p gui"
      end tell
      return
    end try
  end if

  tell application "Terminal"
    activate
    do script shellInit & "p gui"
  end tell
end run

on applicationIsRunning(appName)
  tell application "System Events"
    return (name of processes) contains appName
  end tell
end applicationIsRunning
