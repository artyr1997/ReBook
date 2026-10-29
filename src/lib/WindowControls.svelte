<script lang="ts">
	// Desktop only: the native title bar is transparent and its buttons are hidden
	// (src-tauri/src/lib.rs), so the window gets vertical close / minimise / zoom
	// buttons and an invisible strip along the top for dragging.
	import { getCurrentWindow } from '@tauri-apps/api/window';

	const win = getCurrentWindow();

	// the buttons show only while this window is the focused one of the active app,
	// and then it has no shadow either
	let focused = $state(true);
	$effect(() => {
		win.setShadow(!focused);
	});
	$effect(() => {
		win.isFocused().then((f) => (focused = f));
		const off = win.onFocusChanged(({ payload }) => (focused = payload));
		return () => {
			off.then((stop) => stop());
		};
	});
</script>

<div class="drag-strip" data-tauri-drag-region></div>

<div class="window-controls" class:away={!focused}>
	<button class="light close" onclick={() => win.close()} title="Close" aria-label="Close">
		<svg viewBox="0 0 8 8"><path d="M2 2l4 4M6 2L2 6" /></svg>
	</button>
	<button class="light minimize" onclick={() => win.minimize()} title="Minimize" aria-label="Minimize">
		<svg viewBox="0 0 8 8"><path d="M1.5 4h5" /></svg>
	</button>
	<button class="light zoom" onclick={() => win.toggleMaximize()} title="Zoom" aria-label="Zoom">
		<svg viewBox="0 0 8 8"><path d="M4 1.5v5M1.5 4h5" /></svg>
	</button>
</div>

<style>
	.drag-strip {
		position: fixed;
		top: 0;
		left: 0;
		right: 0;
		height: 28px;
		z-index: 90;
	}

	.window-controls {
		position: fixed;
		top: 14px;
		left: 14px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		z-index: 95;
		transition: opacity 150ms ease;
	}

	.window-controls.away {
		opacity: 0;
		visibility: hidden;
		transition: opacity 150ms ease, visibility 0s 150ms;
	}

	.light {
		width: 12px;
		height: 12px;
		border-radius: 50%;
		border: none;
		padding: 0;
		display: grid;
		place-items: center;
		cursor: default;
		/* grey at rest; the macOS colours appear when hovering the group */
		background: var(--light-off, #c9c9c9);
		box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.12);
		transition: background 150ms ease;
	}

	.window-controls:hover .close { background: #ff5f57; }
	.window-controls:hover .minimize { background: #febc2e; }
	.window-controls:hover .zoom { background: #28c840; }

	/* like macOS: the symbols show when hovering the group */
	.light svg {
		width: 8px;
		height: 8px;
		stroke: rgba(0, 0, 0, 0.55);
		stroke-width: 1.2;
		stroke-linecap: round;
		fill: none;
		opacity: 0;
	}

	.window-controls:hover .light svg {
		opacity: 1;
	}
</style>
