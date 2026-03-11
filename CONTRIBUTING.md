# Contributing to Pane

Thanks for your interest in contributing. Pane is early-stage and there's a lot to build. All skill levels are welcome.

## Getting Started

1. Fork the repository
2. Clone your fork
3. Create a branch for your work (`git checkout -b your-feature`)
4. Make your changes
5. Commit with a clear message
6. Push to your fork and open a pull request

## What We Need Help With

### Right Now
- Testing on different hardware (especially various GPU configurations)
- Testing with different WSL2 distros (Ubuntu, Fedora, Arch, etc.)
- Documentation and guides
- Bug reports

### Coming Soon
- Rust development (Windows launcher, WSL2 daemon)
- XRDP configuration and optimization
- Audio/clipboard integration
- Packaging and distribution

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) (stable, latest)
- Windows 10/11 with WSL2
- A WSL2 distro with a desktop environment installed

### Build

```bash
cargo build
```

### Run

```bash
cargo run -- launch
```

## Code Guidelines

- Write clear, readable code over clever code
- Follow `rustfmt` and `clippy` defaults (configs are in the repo root)
- Keep functions small and focused
- Add comments only where the intent isn't obvious from the code
- Error messages should be actionable — tell the user what to do, not just what went wrong

## Commit Messages

Use clear, concise commit messages:

```
Add GPU detection for iGPU/dGPU selection
Fix XRDP session not reconnecting after sleep
Update README with Fedora setup instructions
```

No particular format is enforced. Just be clear about what changed and why.

## Pull Requests

- Keep PRs focused — one feature or fix per PR
- Describe what your PR does and why
- Link any related issues
- Make sure `cargo clippy` and `cargo fmt --check` pass

## Reporting Bugs

Open an issue with:

1. What you expected to happen
2. What actually happened
3. Your setup (Windows version, WSL2 distro, GPU, desktop environment)
4. Steps to reproduce

## Feature Requests

Open an issue describing:

1. The problem you're trying to solve
2. How you'd like it to work
3. Any alternatives you've considered

## Code of Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Be respectful. We're all here to build something useful.
