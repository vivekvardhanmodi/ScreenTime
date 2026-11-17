# Agent Guidelines for ScreenTime

## Commit Guidelines

- **Format**: Commit messages must follow the `<type>(<scope>): <message>` convention (e.g., `fix(daemon): <message>` or `feature(web): <message>`).
- **Scope**: Scopes are not restricted to `daemon` or `web`. Use any scope that best describes the context of the change (e.g., `mobile`, `database`, `hyprland`, `kde`, `wlroots`, `windows`, `ipc`, or even a specific file or module).
- **Commit Body**: Only write a commit message body if the commit title would otherwise be too long.
- **Brevity & Slice Size**: Needing a commit body does not grant permission to write long commit messages. If a long commit message is needed to describe a change, the change is likely doing too much and should have been split into multiple smaller, broken-down commits within the PR.

## Safety & Verification

- **Personal Database Safety**: Never touch, read, write, or alter the host database at `~/.local/share/screentime/screentime.db`. All tests and automated verification must strictly use ephemeral temporary databases (`tempfile::tempdir()`).
- **Resource Limits**: Restrict Cargo concurrency (e.g., pass `-j 2` or keep `CARGO_BUILD_JOBS=2`) to prevent CPU starvation on the host machine.
