# Convenience entry points for macOS / Linux. Windows has no `make` by default; use the
# plain commands in README "本地开发" there.
#
# Rust targets need OpenCV (README "OpenCV 构建环境"): load the environment first, e.g.
#   source /tmp/ocvenv.sh

WEB := apps/desktop/web

.PHONY: dev-web dev-desktop check check-web check-rust fmt fmt-check build-web build-desktop

# Browser-only UI work: uses the mock backend, so no screenshots and no clicks.
dev-web:
	pnpm --dir $(WEB) dev

dev-desktop: require-opencv
	cd apps/desktop && ./web/node_modules/.bin/tauri dev

# What CI runs. The web build comes before the Rust checks because the Tauri crate embeds
# apps/desktop/web/dist at compile time.
check: fmt-check check-web check-rust

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

check-web:
	pnpm --dir $(WEB) typecheck
	pnpm --dir $(WEB) test
	pnpm --dir $(WEB) build

# Clippy reports warnings without failing: the workspace has ~12 existing ones (in
# liverguard-vision, liverguard-engine, the desktop crate and liverguard-driver-win) that
# nobody has decided to fix yet. Add `-- -D warnings` once they are gone.
check-rust: require-opencv
	cargo clippy --workspace --all-targets
	cargo test --workspace --all-targets

build-web:
	pnpm --dir $(WEB) build

# Local macOS build signed with the stable self-signed identity (keeps the Screen Recording
# and Accessibility grants across rebuilds). Release installers come from GitHub Actions.
build-desktop: require-opencv
	scripts/mac-dev-build.sh

.PHONY: require-opencv
require-opencv:
	@if [ -z "$$OpenCV_DIR" ]; then \
		echo "OpenCV_DIR is not set. Load the OpenCV environment first (AGENTS.md: source /tmp/ocvenv.sh;" >&2; \
		echo "if the file is missing, rebuild it with OpenCV_DIR, CMAKE_PREFIX_PATH, OPENCV_DISABLE_PROBES)." >&2; \
		exit 1; \
	fi
