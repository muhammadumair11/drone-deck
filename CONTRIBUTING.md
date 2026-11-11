# Contributing to Drone Deck

Drone Deck is an early Windows desktop workspace for drone autonomy development. Contributions, reproducible bug reports and documentation improvements are welcome.

## Scope and layout

- drone-deck/: application, runtime helpers, installer and application documentation.
- crates/, examples/, themes/, site/: bundled Rune framework and upstream examples. Keep app changes in drone-deck/ unless a framework change is necessary.
- [Known issues](drone-deck/docs/known-issues.md): existing bugs and workflows awaiting real-environment verification.
- Root LICENSE: MIT; preserve the Rune/Anomaly notices and identify your contributions accurately.

The desktop interface supports Windows 10/11 x64. Linux flight tools run in WSL 2, initially Ubuntu 24.04. Native Linux Drone Deck is outside current scope.

## Development

Install the pinned Rust toolchain (rust-toolchain.toml) and Microsoft C++ Build Tools with a Windows SDK. From the repository root:

~~~powershell
cargo fmt --manifest-path drone-deck/Cargo.toml --check
cargo test --locked --manifest-path drone-deck/Cargo.toml
cargo run --manifest-path drone-deck/Cargo.toml
~~~

Tests use generated data, fake agents and loopback vehicles; they do not establish real simulator or hardware compatibility. Headless preview does not launch external tools:

~~~powershell
cargo build --locked --manifest-path drone-deck/Cargo.toml
drone-deck\target\debug\drone-deck.exe --scene tools --snap tools.png --size 1280x800
~~~

Helper tests need Python 3.12 in Linux/WSL with pyyaml, mcap, mcap-ros2-support and numpy. Use the test commands in .github/workflows/drone-deck.yml as the authoritative CI recipe. Inno Setup 6.3+ is needed only to build the installer.

## Pull requests

1. Check existing issues and Known issues. Open an issue before substantial redesigns or new integrations.
2. Create a focused branch from main. Explain the problem, resulting behavior and checks performed.
3. Add meaningful regression coverage for behavior changes. UI changes should include screenshots and relevant window sizes.
4. Add a short user-facing changelog entry.
5. Run formatting and relevant tests. Distinguish code inspection, automated tests and actual integration checks.
6. Preserve saved configurations and custom commands; document migrations and compatibility changes.

Use simulated vehicles for development. Changes affecting physical-vehicle command routing need explicit review of the authority boundary.

## Reports and discussions

Use the bug and feature-request forms. Include Windows, WSL/distribution and tool versions, reproduction steps, expected behavior and redacted diagnostics. Review attachments for tokens, personal paths and vehicle/location data.

Be respectful and give actionable feedback. Maintainers may remove abusive content and close submissions outside project scope.

See [SECURITY.md](SECURITY.md) for security reports and [SUPPORT.md](SUPPORT.md) for support boundaries.
