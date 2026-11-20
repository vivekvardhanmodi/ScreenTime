# 04: Event-Driven Sleep/Suspend and Clamshell-Aware Lid Handling

**What to build:**
1. On Linux, hook into `org.freedesktop.login1.Manager` system D-Bus signals via `zbus`:
   - Intercept `PrepareForSleep(true)` to cleanly close the active session in SQLite before system sleep.
   - Intercept `PrepareForSleep(false)` to resume tracking upon system wakeup.
   - Listen to `LidClosed` property changes. If closed and no external display is connected, pause tracking. If an external display is active, continue tracking (clamshell mode).
2. On Windows, handle system power broadcast suspend/resume notifications in the Windows event loop.
3. Eliminate time leakage during short suspends (<30s).

**Blocked by:** 02

**Status:** open

- [ ] Implement `LogindSleepListener` using `zbus` on Linux system bus
- [ ] Implement clamshell display check before pausing on lid closure
- [ ] Connect sleep/wake events to `SessionManager` state machine
- [ ] Implement Windows power suspend notification handler
- [ ] Integration tests verifying clean session close and wake-up session creation
