<script lang="ts">
	// Modal in the ReNote style: the window behind is blurred, the box is paper.
	// Keys (Enter / Esc) are handled by the page so they work the same everywhere.
	import type { Snippet } from 'svelte';

	let { onclose, children }: { onclose: () => void; children: Snippet } = $props();
</script>

<div class="overlay" onclick={onclose} aria-hidden="true"></div>
<div class="box" role="dialog" aria-modal="true">
	{@render children()}
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 60;
		backdrop-filter: blur(6px);
		-webkit-backdrop-filter: blur(6px);
	}

	.box {
		position: fixed;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
		background: var(--bg) url('/noise.svg') repeat;
		border: 2px solid var(--line);
		padding: 2.5rem 3rem;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 1.5rem;
		z-index: 70;
		min-width: 320px;
		max-width: min(560px, calc(100vw - 4rem));
		max-height: 80vh;
		overflow-y: auto;
	}

	/* shared bits for popup contents */
	.box :global(.pop-heading) {
		font-family: 'Noto Sans JP', sans-serif;
		font-size: 18px;
		letter-spacing: 0.4em;
		margin-right: -0.4em;
		color: var(--text);
	}

	.box :global(.pop-divider) {
		width: 100%;
		height: 1px;
		background: var(--line);
		opacity: 0.3;
	}

	.box :global(.pop-text) {
		font-family: 'Kurale', serif;
		font-size: 18px;
		text-align: center;
		overflow-wrap: anywhere;
	}

	.box :global(.pop-actions) {
		display: flex;
		gap: 2rem;
	}

	.box :global(.pop-btn) {
		background: transparent;
		border: 2px solid var(--line);
		font-family: 'Noto Sans JP', sans-serif;
		font-size: 16px;
		letter-spacing: 0.2em;
		padding: 0.4rem 1.2rem;
		cursor: pointer;
		transition: background 200ms ease, color 200ms ease;
	}

	.box :global(.pop-btn:hover) {
		background: var(--line);
		color: var(--bg);
	}
</style>
