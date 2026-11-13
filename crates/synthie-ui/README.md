# synthie-ui

The shared neon look of the [synthie](https://github.com/sunsided/synthie)
terminal front-ends, for [ratatui](https://ratatui.rs/): the palette and
gradients, rounded panels, the animated wordmark, key-cap hints, meters and
a level meter, a scope buffer, a quit/command modal, and a backend-neutral
`Key` type.

Used by `synth-tui`, `synthie-tracker` and
[synthie-trombone](https://github.com/sunsided/synthie-trombone).

## Features

- `crossterm` (default): `From<crossterm::event::KeyEvent>` for `Key`.
- `web`: `From<&web_sys::KeyboardEvent>` for `Key`, plus the `web` module
  (WebGL2 terminal via Ratzilla, centring, start prompt, key listeners) for
  browser builds.

## License

EUPL-1.2.
