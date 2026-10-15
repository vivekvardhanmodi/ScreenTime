## 2025-02-24 - [CRITICAL] Path Traversal in API
**Vulnerability:** Path traversal in `catch_all` endpoint allowing attackers to escape `WEB_DIR`.
**Learning:** `os.path.abspath(WEB_DIR)` concatenated with `startswith` validation enables attackers to access files starting with the same prefix. E.g. `/app/dist_secret` starts with `/app/dist`.
**Prevention:** Always use `os.path.commonpath([base_dir, target_dir]) == base_dir` for directory constraint checks rather than string-based prefix checking.
