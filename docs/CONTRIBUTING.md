# Contributing to Dither

Thank you for your interest in contributing to Dither! This document provides guidelines for development, code style, and testing.

## Table of Contents

1. [Setting Up](#setting-up)
2. [Development Workflow](#development-workflow)
3. [Code Style](#code-style)
4. [Testing](#testing)
5. [Commit Guidelines](#commit-guidelines)
6. [Architecture](#architecture)
7. [Common Tasks](#common-tasks)

## Setting Up

### Prerequisites

- **Rust 1.70+**: Install from [rustup.rs](https://rustup.rs/)
- **Node.js 18+**: Install from [nodejs.org](https://nodejs.org/)
- **Git**: For version control

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/yourusername/dither-yuki-2.git
cd dither-yuki-2

# Install Rust dependencies
cargo build --all

# Install frontend dependencies
npm install

# Verify everything works
cargo test --all
npm run build --workspace=frontend
```

## Development Workflow

### Running the Application

**Integrated mode** (recommended for frontend development):
```bash
npm run tauri:dev
```

This starts:
1. Vite dev server on `http://localhost:5173` (with hot-reload)
2. Tauri application connected to dev server

**Manual two-terminal setup** (if you need more control):

Terminal 1 (frontend):
```bash
npm run dev --workspace=frontend
```

Terminal 2 (backend):
```bash
npm run tauri:dev
# or
cargo run -p dither
```

### Branches

- **main**: Stable, production-ready code
- **develop**: Integration branch for features
- **feature/**: For new features (branch off `develop`)
- **fix/**: For bug fixes (branch off `main` or `develop`)

**Example**:
```bash
git checkout develop
git pull origin develop
git checkout -b feature/tile-cache-optimization
```

### Before You Commit

1. **Format code**:
   ```bash
   cargo fmt --all
   ```

2. **Run linter**:
   ```bash
   cargo clippy --all -- -D warnings
   ```

3. **Run tests**:
   ```bash
   cargo test --all
   npm run build --workspace=frontend
   ```

4. **Check documentation**:
   ```bash
   cargo doc --all --no-deps
   ```

## Code Style

### Rust

Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).

**Key points**:
- Use `cargo fmt` for formatting (enforced by CI)
- No warnings allowed: `clippy` runs with `-D warnings`
- Descriptive variable names (avoid abbreviations)
- Document public APIs with `///` comments

**Example**:
```rust
/// Processes a tile at the given coordinates.
///
/// # Arguments
/// * `coords` - The tile coordinates (level, x, y)
/// * `max_zoom` - Maximum zoom level
///
/// # Returns
/// A `Result<Tile>` containing the processed tile or an error
pub fn process_tile(coords: TileCoords, max_zoom: u8) -> Result<Tile> {
    // Implementation
}
```

**Module documentation**:
```rust
//! Core data model for the Dither engine.
//!
//! This module defines the fundamental types used throughout the application:
//! - Layer: A single layer with pixels and metadata
//! - Document: A collection of layers
//! - Filter: An effect applied to a layer
//! - BlendMode: How layers composite together
```

### TypeScript/React

Follow [Airbnb JavaScript Style Guide](https://github.com/airbnb/javascript).

**Key points**:
- Use `const` by default, `let` only if needed
- Functional components with hooks (no class components)
- Descriptive prop types with TypeScript
- One component per file (except small utilities)

**Example**:
```typescript
import React from 'react'

interface LayerPanelProps {
  layers: Layer[]
  onLayerSelect: (id: string) => void
}

export const LayerPanel: React.FC<LayerPanelProps> = ({
  layers,
  onLayerSelect,
}) => {
  return (
    <div className="layer-panel">
      {layers.map((layer) => (
        <LayerItem
          key={layer.id}
          layer={layer}
          onClick={() => onLayerSelect(layer.id)}
        />
      ))}
    </div>
  )
}
```

### File Organization

**Rust**:
```
crates/engine-tiles/src/
├── lib.rs              # Public API, module declarations
├── cache.rs            # TileCache implementation
├── pyramid.rs          # Pyramid structure
├── scheduler.rs        # Task scheduler
└── tests/              # Integration tests (if needed)
```

**TypeScript**:
```
frontend/src/
├── components/         # React components
│   ├── Canvas.tsx      # Canvas rendering
│   └── LayerPanel.tsx
├── hooks/              # Custom React hooks
│   └── useImageData.ts
├── types/              # TypeScript interfaces
│   └── index.ts
├── App.tsx             # Root component
├── main.tsx            # Entry point
└── index.css           # Global styles
```

## Testing

### Unit Tests

**Rust**:
```bash
cargo test --all
cargo test -p engine-tiles
cargo test -p dither -- --test-threads=1  # Single-threaded
```

**TypeScript**:
```bash
# To be set up in Phase 1
npm run test
```

### Writing Tests

**Rust example**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_cache_insert_and_retrieve() {
        let mut cache = TileCache::new(100);
        let tile = Tile::new(0, 0, 0);
        
        cache.insert(tile.key(), tile.clone());
        assert_eq!(cache.get(&tile.key()), Some(&tile));
    }

    #[test]
    #[should_panic]
    fn test_invalid_zoom_level() {
        let _ = TileKey::new(0, 0, 40);  // Max zoom is typically 28
    }
}
```

### Test Organization

- Place unit tests in the same file (with `#[cfg(test)]` module)
- Put integration tests in `tests/` directory at crate root
- Use descriptive test names: `test_feature_scenario_outcome`

### Performance Testing

```bash
# Run benchmarks
cargo bench --all

# Profile memory usage
cargo build --release
heaptrack ./target/release/dither
```

## Commit Guidelines

### Message Format

```
type(scope): Brief description (50 chars max)

Longer explanation (72 chars per line max)
- Use bullet points for multiple changes
- Reference issues: Fixes #123

Related: issue-tracking-id
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation
- `refactor`: Code refactoring (no functional change)
- `perf`: Performance improvement
- `test`: Test addition or update
- `build`: Build system or dependencies
- `ci`: CI/CD changes

**Example**:
```
feat(tile-cache): Implement LRU eviction policy

- Add LRU tracking to TileCache
- Track access time for each tile
- Evict least-recently-used tiles when capacity exceeded
- Reduces memory usage by 40% on large images

Fixes #42
```

### Before Pushing

```bash
# Rebase on main/develop
git rebase origin/develop

# Verify tests still pass
cargo test --all

# View what you're about to push
git log origin/develop..HEAD

# Push
git push origin feature/your-feature-name
```

## Architecture

### Key Principles

1. **Separation of Concerns**: Each crate handles one responsibility
2. **Async-First**: Use tokio for all I/O operations
3. **Zero-Copy Where Possible**: Use `Arc` for shared data
4. **Immutable by Default**: Use `const` and `&` references

### Module Dependencies

```
frontend/
└── Tauri Commands
    └── crates/app
        ├── engine-core (Layer, Document)
        ├── engine-tiles (TileCache)
        ├── engine-color (Color processing)
        ├── engine-io (File I/O)
        └── engine-project (Storage)
```

- **No circular dependencies** between crates
- **engine-core** is independent (foundation)
- **engine-tiles** depends on engine-core
- **Other engines** depend on engine-core only

## Common Tasks

### Adding a New Crate

```bash
# Create directory
mkdir crates/engine-new

# Create Cargo.toml
cat > crates/engine-new/Cargo.toml << 'EOF'
[package]
name = "engine-new"
version = "0.1.0"
edition = "2021"

[dependencies]
engine-core = { path = "../engine-core" }
EOF

# Create source
mkdir -p crates/engine-new/src
cat > crates/engine-new/src/lib.rs << 'EOF'
//! Description of this module.

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}
EOF

# Add to workspace Cargo.toml
# Update members list in Cargo.toml

# Verify
cargo build -p engine-new
```

### Adding a New Feature

```bash
# 1. Create feature branch
git checkout -b feature/my-feature

# 2. Write tests first (TDD)
# 3. Implement feature
# 4. Update documentation

# 5. Format and lint
cargo fmt --all
cargo clippy --all -- -D warnings

# 6. Run all tests
cargo test --all

# 7. Commit and push
git add .
git commit -m "feat(module): Add my feature"
git push origin feature/my-feature

# 8. Create pull request on GitHub
```

### Debugging

**Print debugging**:
```rust
dbg!(variable);  // Prints with file:line info
println!("Debug: {:?}", variable);
```

**Logging with tokio**:
```bash
RUST_LOG=debug cargo run -p dither
```

**Browser DevTools** (frontend):
- F12 or Cmd+Option+I (macOS)
- Console tab for JavaScript errors
- Sources tab for breakpoints

### Profiling

**CPU Profiling** (macOS with Instruments):
```bash
cargo build --release
instruments -t "CPU Profiler" -l 30 ./target/release/dither
```

**Memory Profiling** (Linux with heaptrack):
```bash
cargo build --release
heaptrack ./target/release/dither
heaptrack_gui heaptrack.dither.*
```

### Documentation

**Generate and view docs**:
```bash
cargo doc --all --no-deps --open
```

**Update README**:
- Keep high-level architecture current
- Add quick-start steps for new features
- Reference architecture docs

## CI/CD

The project uses GitHub Actions (to be configured). Tests must pass before merging:

```yaml
- Compile: cargo build --all
- Lint: cargo clippy --all -- -D warnings
- Test: cargo test --all
- Frontend: npm run build
```

## Questions?

- **Architecture**: See docs/ folder and code comments
- **Design decisions**: Check git history with `git log -p`
- **Specific issues**: File an issue on GitHub

---

**Last Updated**: July 27, 2024
**Phase**: 0
