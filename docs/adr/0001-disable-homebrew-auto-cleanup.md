# Disable Homebrew's automatic cleanup

Homebrew normally deletes a Package's old versions straight after upgrading it. A process that is still running from the old version can then lose its files or libraries, which is exactly the data-loss risk brewmax exists to prevent. brewmax therefore runs every `brew` command with automatic cleanup turned off (`HOMEBREW_NO_INSTALL_CLEANUP=1`). Old versions are removed only by brewmax's own Cleanup, which shows its Plan for confirmation and skips anything still In Use.

## Consequences

Old versions take up disk space until you run Cleanup. Running `brew` directly outside brewmax still triggers Homebrew's normal cleanup.
