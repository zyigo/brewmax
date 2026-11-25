# brewmax

A terminal UI for managing the Homebrew-installed software on one Mac without disrupting software that is currently running.

## Language

### Packages

**Package**:
Anything Homebrew installs and brewmax manages: either a Cask or a Formula.
_Avoid_: App (only fits Casks), program, software

**Cask**:
A Package that is a macOS GUI application, typically living in `/Applications`.
_Avoid_: App, GUI package

**Formula**:
A Package that is a command-line tool, library or service.
_Avoid_: CLI package, brew package

**Self-Updating**:
A Cask that updates itself in place, so Homebrew does not normally treat it as outdated.
_Avoid_: Auto-update cask, greedy

### Safety

**In Use**:
A Package that something running depends on right now: a Cask whose app is running, or a Formula with a running process or service.
_Avoid_: Open, active, running

**Held**:
A Package that brewmax deliberately left unchanged because it was In Use.
_Avoid_: Skipped, blocked, deferred

**Graceful Quit**:
Asking an In Use Cask to quit the normal way, so it can ask to save unsaved work. Never a force-kill.
_Avoid_: Kill, close, terminate

**Service Restart**:
The Formula counterpart of a Graceful Quit: stopping an In Use service, changing it, then starting it again.
_Avoid_: Bounce, kick

### Operations

**Plan**:
The full set of Package changes an operation would make, including dependencies Homebrew would change along the way.
_Avoid_: Job, batch, transaction

**Cleanup**:
Removing old Package versions, cached downloads and dependencies nothing needs any more, while leaving anything In Use untouched.
_Avoid_: Prune, purge, autoremove

**Zap**:
Removing a Cask together with its settings, caches and data. More destructive than an ordinary uninstall, and not reversible.
_Avoid_: Purge, deep uninstall, nuke
