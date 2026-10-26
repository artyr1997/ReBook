<script lang="ts">
	// One PDF page drawn sharp at the given display size (re-rendered when it changes).
	import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist';

	let { doc, index, width, height }: { doc: PDFDocumentProxy; index: number; width: number; height: number } = $props();

	let canvas: HTMLCanvasElement | undefined = $state();

	$effect(() => {
		if (!canvas || width <= 0) return;
		const target = canvas;
		let task: RenderTask | null = null;
		let cancelled = false;
		doc.getPage(index + 1).then(page => {
			if (cancelled) return;
			const dpr = window.devicePixelRatio || 1;
			const viewport = page.getViewport({ scale: (width / page.getViewport({ scale: 1 }).width) * dpr });
			target.width = Math.round(viewport.width);
			target.height = Math.round(viewport.height);
			const ctx = target.getContext('2d')!;
			ctx.fillStyle = '#fff';
			ctx.fillRect(0, 0, target.width, target.height);
			task = page.render({ canvasContext: ctx, viewport });
			task.promise.catch(() => {});
		});
		return () => {
			cancelled = true;
			task?.cancel();
		};
	});
</script>

<canvas bind:this={canvas} style="width: {width}px; height: {height}px;"></canvas>

<style>
	canvas {
		display: block;
		box-shadow: 0 1px 12px rgba(0, 0, 0, 0.08);
	}
</style>
