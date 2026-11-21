# ScreenTime Domain Glossary

ScreenTime tracks active application and website usage across multiple desktop platforms, persisting time sessions to a local SQLite database.

## Language

**Daemon**:
The background service (`screentimed`) that tracks active window changes, idle state transitions, and browser URLs to maintain session records in SQLite.
_Avoid_: Agent, server, worker

**Session**:
A contiguous, non-overlapping duration during which a single window and optional browser URL was active without the user going idle.
_Avoid_: Activity interval, time slice, event log

**WindowTracker**:
A platform- or compositor-specific component responsible for querying and reporting focus changes to the active window.
_Avoid_: Focus monitor, window watcher, app hook

**IdleDetector**:
A platform- or compositor-specific component responsible for detecting user inactivity, screen saver state, and display power (DPMS) toggles.
_Avoid_: Sleep monitor, inactivity timer

**BrowserHost**:
A native messaging host process communicating with browser extensions (Chrome, Firefox) over stdin/stdout length-prefixed JSON to convey the current tab's active URL.
_Avoid_: URL forwarder, extension proxy

**TitleRule**:
A user-configured prefix/pattern rule used to reclassify an application identity based on the window title (e.g. splitting terminal or browser windows into sub-identities).
_Avoid_: Alias, filter rule, window remap
