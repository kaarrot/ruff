# Testing Push Notification Capabilities

This document explains how to test the push notification capabilities added to the Ty server.

## Overview

The following push notification capabilities were added:

1. **Automatic diagnostic publishing on file open** (`did_open.rs`)
2. **Automatic diagnostic publishing on file change** (`did_change.rs`)  
3. **Conditional behavior based on client capabilities** (only sends push notifications when pull diagnostics are NOT supported)
4. **Shared diagnostic computation** (`diagnostics.rs`)

## Running Tests

### Unit Tests

```bash
# Run all tests in the ty_server crate
cargo test -p ty_server

# Run only push notification tests
cargo test -p ty_server push_notification_tests

# Run diagnostic-specific tests  
cargo test -p ty_server diagnostic_tests
```

### Test Coverage

The tests cover:

- **Client capability detection**: Verifies that `pull_diagnostics` is correctly parsed from client capabilities
- **Push notification logic**: Tests the decision logic for when to send push notifications
- **Diagnostic format**: Validates diagnostic structure and content
- **Conditional behavior**: Ensures notifications are only sent when appropriate

## What the Tests Verify

### 1. Client Capability Parsing (`test_pull_diagnostics_capability_detection`)

Tests that the server correctly determines whether a client supports pull diagnostics:

- ✅ Client WITH diagnostic capability → `pull_diagnostics = true`
- ✅ Client WITHOUT diagnostic capability → `pull_diagnostics = false`
- ✅ Client with no text_document capability → `pull_diagnostics = false`

### 2. Push Notification Logic (`test_should_send_push_diagnostics_logic`)

Tests the core decision logic:

- ✅ When `pull_diagnostics = false` → should send push notifications
- ✅ When `pull_diagnostics = true` → should NOT send push notifications

### 3. Conditional Behavior (`test_notification_behavior_based_on_capabilities`)

Tests that the logic works end-to-end:

- ✅ Only sends notifications when appropriate
- ✅ Skips notifications when client supports pull diagnostics

### 4. Diagnostic Format Tests (`diagnostic_tests`)

Tests that diagnostics have the correct structure:

- ✅ Valid range positions
- ✅ Proper severity levels (ERROR, WARNING, INFORMATION)
- ✅ Source field set to "ty"
- ✅ Non-empty, meaningful messages
- ✅ Valid diagnostic codes

## Manual Testing

To manually test with a real LSP client:

### Client WITHOUT Pull Diagnostics Support

Configure your LSP client to NOT include diagnostic capabilities:

```json
{
  "textDocument": {
    // diagnostic field omitted or set to null
  }
}
```

**Expected behavior:**
- Opening a Python file with errors → receive `textDocument/publishDiagnostics` notification
- Editing a file to introduce errors → receive `textDocument/publishDiagnostics` notification
- Fixing errors → receive `textDocument/publishDiagnostics` with empty diagnostics array

### Client WITH Pull Diagnostics Support

Configure your LSP client to include diagnostic capabilities:

```json
{
  "textDocument": {
    "diagnostic": {
      "dynamicRegistration": true
    }
  }
}
```

**Expected behavior:**
- Opening/editing files → NO automatic `textDocument/publishDiagnostics` notifications
- Client must request diagnostics via `textDocument/diagnostic` requests

## Debugging

Enable debug logging to see notification behavior:

```bash
RUST_LOG=debug cargo run -- server --preview
```

Look for log messages related to:
- Client capability parsing
- Diagnostic computation
- Push notification decisions

## Integration with Real Editors

### VS Code
The extension would need to configure client capabilities appropriately.

### Neovim
```lua
local client_capabilities = vim.lsp.protocol.make_client_capabilities()
-- To disable pull diagnostics:
client_capabilities.textDocument.diagnostic = nil
```

### Emacs
Configure lsp-mode to disable pull diagnostic capabilities.

## Performance Notes

The implementation:
- Only computes diagnostics when needed (client doesn't support pull)
- Reuses diagnostic computation logic between push and pull
- Sends notifications asynchronously
- Properly handles document versioning

## Common Issues

1. **Notifications sent when they shouldn't be**: Check client capability parsing
2. **Notifications not sent when expected**: Verify client doesn't advertise diagnostic support
3. **Incorrect diagnostic content**: Check diagnostic computation and conversion
4. **Version mismatches**: Ensure document versions are handled correctly 