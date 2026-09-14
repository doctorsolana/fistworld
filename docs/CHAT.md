# In-game chat

Press **T** while playing to open server chat. **Enter** sends one message and
closes the composer; press T again for the next message. **Escape** closes without
sending and keeps the draft. Up to three compact two-line previews stay visible for ten real seconds,
then fade over three seconds. Reopening shows the last 100 messages received in
this connection. There is no persistent log or replay for players joining later.
When no message or draft is being shown, the panel disappears completely; there
is no persistent T/chat hint on the HUD.

The panel reuses the HUD's wood/brass frame, shared fonts and reveal motion. Names
are gold and message text is cream. It sits above the selection card, to the left
of the army dock. Modals and the arrival cinematic hide chat. Closed previews also
yield to a building inspector. Typing owns gameplay input without hiding the HUD;
Enter/Escape retain ownership for their closing frame, and physical key releases
are never discarded. Losing native window focus closes the composer and keeps
the draft. The editor supports Unicode, selection, clipboard operations, cursor
navigation and horizontal caret following.

## Ownership and limits

| Module | Responsibility |
| --- | --- |
| `shared/src/protocol/chat.rs` | Wire types, text limits, bounded binary decoding and validation |
| `server/src/net/chat.rs` | Accepted-account identity, rate limits, ordering and delivery |
| `client/src/ui/chat/state.rs` | Bounded scrollback, pending acknowledgments and connection reset |
| `client/src/ui/chat/draft.rs`, `input.rs` | Draft editing and native input ownership |
| `client/src/ui/chat/network.rs` | Production reliable send/receive adapters |
| `client/src/ui/chat/view.rs` | Retained panel, text binding, scrollback and fading |

Messages are one plaintext line, at most 280 Unicode scalar values and 1024 UTF-8
bytes. Controls, newlines and directional overrides are rejected. The server
supplies the saved account display name; clients cannot impersonate a sender in
their message payload. Only currently accepted connections receive broadcasts.

Chat has a dedicated bidirectional ordered reliable channel. Its protocol ID is
`0x1234567890ABCE0A`; update both client and server together. Processing uses real
time in `Update`, independent of world pause or time warp. Each connection gets a
three-message burst and refills one message every two seconds. The handler
inspects at most four requests per connection per pass, drains overflow, and
replies to each inspected valid-channel request so a normal client never waits
forever on a deliberately suppressed rejection.

The client allows one pending send, appends only the authoritative echo, and keeps
the draft until acceptance. A later edit is preserved when an earlier echo
arrives. Rejection keeps the draft and shows inline feedback. Disconnect clears
scrollback and pending state; it never automatically resends a draft. Server work
visits connection entities only, without NPC, terrain or region searches. History
rows update on new messages; the only continuously fading rows are the three
recent previews. This adds no image assets or render cameras.

## Verification

Run `cargo check --workspace --all-targets`, shared chat tests, server chat packet
tests and client tests. `capture/chat_session.py` exercises two actual clients on
an owned local server, using normal account/character creation and native input.
See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md#connected-chat) for the repeatable run.
Team/local channels, private messages, moderation tools and persistent history
are not implemented.
