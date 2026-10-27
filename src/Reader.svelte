<script lang="ts">
	// Reading view. PDFs and comics are page-based: portrait pages come two per
	// screen, a landscape page takes the whole screen (see buildSpreads).
	// EPUB text reflows; epub.js shows two columns when the window is wide.
	import { onMount, untrack } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import type { PDFDocumentProxy } from 'pdfjs-dist';
	import type { Book as EpubBook, Rendition, Location } from 'epubjs';
	import PdfPage from './lib/PdfPage.svelte';
	import { openPdf, openEpub, epubFontCss } from './lib/formats';
	import {
		library, urls, isComic, buildSpreads, spreadOf, fitSpread, pageLabel,
		type Book, type PageSize,
	} from './lib/books';

	let { book, onclose }: { book: Book; onclose: () => void } = $props();

	let paged = $derived(book.format !== 'epub');
	const win = getCurrentWindow();

	let error = $state('');
	let stageWidth = $state(0);
	let stageHeight = $state(0);

	// ── page-based (PDF, comics) ──
	let sizes: PageSize[] = $state([]);
	let spreads: number[][] = $state([]);
	let current = $state(0);
	let pdf: PDFDocumentProxy | null = $state.raw(null);

	// manga: pages run right to left, so a spread shows its second page on the left
	// starts from the book's saved setting, then it's toggled here
	let rtl = $state(untrack(() => book.rtl));
	let spread = $derived(spreads[current] ?? []);
	let shown = $derived(rtl ? [...spread].reverse() : spread);
	let fitted = $derived(
		shown.length && stageWidth ? fitSpread(shown.map(i => sizes[i]), { width: stageWidth, height: stageHeight }) : [],
	);

	function toggleManga() {
		rtl = !rtl;
		library.setRtl(book.id, rtl);
	}

	// which way the left / right arrows and sides turn depends on the reading direction
	function goLeft() {
		if (rtl) next();
		else prev();
	}
	function goRight() {
		if (rtl) prev();
		else next();
	}

	// ── EPUB ──
	let epubHost: HTMLDivElement | undefined = $state();
	let epub: EpubBook | null = null;
	let rendition: Rendition | null = null;
	let epubLabel = $state('');

	let label = $derived(paged ? pageLabel(spread, sizes.length) : epubLabel);

	// ── saving the place ──
	let saveTimer: ReturnType<typeof setTimeout> | null = null;
	function save(position: string, progress: number) {
		if (saveTimer) clearTimeout(saveTimer);
		saveTimer = setTimeout(() => library.setProgress(book.id, position, progress), 400);
	}

	$effect(() => {
		if (!paged || !spread.length) return;
		save(String(spread[0]), (spread[spread.length - 1] + 1) / sizes.length);
	});

	// preload the next comic spread so turning the page is instant
	$effect(() => {
		if (!isComic(book.format)) return;
		for (const i of spreads[current + 1] ?? []) new Image().src = urls.page(book.id, i);
	});

	function next() {
		if (paged) current = Math.min(current + 1, spreads.length - 1);
		else rendition?.next();
	}

	// going back from the first page returns to the library
	function prev() {
		if (paged) {
			if (current === 0) close();
			else current -= 1;
		} else if ((rendition?.location as { atStart?: boolean } | undefined)?.atStart) {
			close();
		} else {
			rendition?.prev();
		}
	}

	async function toggleFullscreen() {
		await win.setFullscreen(!(await win.isFullscreen()));
	}

	async function close() {
		if (await win.isFullscreen()) await win.setFullscreen(false);
		onclose();
	}

	function handleKey(e: KeyboardEvent) {
		if (e.metaKey || e.ctrlKey || e.altKey) return;
		if (e.key === 'ArrowRight') {
			e.preventDefault();
			goRight();
		} else if (e.key === 'ArrowLeft') {
			e.preventDefault();
			goLeft();
		} else if (e.key === 'PageDown' || e.key === ' ') {
			e.preventDefault();
			next();
		} else if (e.key === 'PageUp') {
			e.preventDefault();
			prev();
		} else if (e.code === 'KeyM' && paged) {
			toggleManga();
		} else if (e.key === 'Home' && paged) {
			current = 0;
		} else if (e.key === 'End' && paged) {
			current = Math.max(spreads.length - 1, 0);
		} else if (e.key === 'Escape') {
			win.isFullscreen().then(full => (full ? win.setFullscreen(false) : onclose()));
		} else if (e.code === 'KeyF') {
			toggleFullscreen();
		}
	}

	async function openPaged() {
		if (book.format === 'pdf') {
			const doc = await openPdf(book.id);
			const list: PageSize[] = [];
			for (let i = 1; i <= doc.numPages; i++) {
				const v = (await doc.getPage(i)).getViewport({ scale: 1 });
				list.push({ width: v.width, height: v.height });
			}
			pdf = doc;
			sizes = list;
		} else {
			sizes = await library.comicPages(book.id);
		}
		spreads = buildSpreads(sizes);
		current = spreadOf(spreads, Number(book.position ?? 0) || 0);
	}

	async function openReflowable() {
		const b = await openEpub(book.id);
		epub = b;
		// `gap` is supported by epub.js but missing from its type definitions
		const options = {
			width: '100%',
			height: '100%',
			flow: 'paginated',
			spread: 'auto',
			minSpreadWidth: 800,
			// pages of a book sit a little closer than epub.js's default (~1/12 of the width)
			gap: 48,
			allowScriptedContent: false,
		};
		const r = b.renderTo(epubHost!, options as Parameters<typeof b.renderTo>[1]);
		rendition = r;
		const fontCss = URL.createObjectURL(new Blob([epubFontCss()], { type: 'text/css' }));
		r.hooks.content.register((contents: { addStylesheet: (url: string) => void }) => contents.addStylesheet(fontCss));
		r.themes.default({
			body: {
				'font-family': "'Kurale', serif !important",
				color: '#474747 !important',
				background: 'transparent !important',
				'line-height': '1.75 !important',
			},
			'p, li, blockquote': { 'font-family': "'Kurale', serif !important" },
			a: { color: '#474747 !important' },
			img: { 'max-width': '100% !important' },
		});
		r.on('keydown', handleKey);
		r.on('relocated', (loc: Location) => {
			const percent = b.locations.length() ? b.locations.percentageFromCfi(loc.start.cfi) : loc.start.percentage;
			epubLabel = b.locations.length()
				? `${loc.start.location + 1} / ${b.locations.length()}`
				: `${Math.round((percent || 0) * 100)}%`;
			save(loc.start.cfi, percent || 0);
		});
		await r.display(book.position || undefined);
		// page numbers for the whole book take a moment; the label switches from % when ready
		b.locations.generate(1200).then(() => {
			const loc = r.currentLocation() as unknown as Location;
			if (loc?.start) r.emit('relocated', loc);
		});
	}

	onMount(() => {
		(paged ? openPaged() : openReflowable()).catch(e => (error = String(e)));
		return () => {
			if (saveTimer) clearTimeout(saveTimer);
			pdf?.destroy();
			epub?.destroy();
		};
	});

	// EPUB pages follow the window size
	$effect(() => {
		if (rendition && stageWidth && stageHeight) rendition.resize(stageWidth, stageHeight);
	});
</script>

<svelte:window onkeydown={handleKey} />

<p class="title" title={book.title}>{book.title}</p>

<button class="side boxed nav-back" onclick={close} title="Library (Esc)">BACK</button>
<button class="side boxed nav-full" onclick={toggleFullscreen} title="Full screen (F)">FULL</button>
{#if paged}
	<button class="side switch nav-manga" class:ink-mark-v={rtl} onclick={toggleManga} title="Manga: right to left (M)">MANGA</button>
{/if}

<main class="stage fade-in" bind:clientWidth={stageWidth} bind:clientHeight={stageHeight}>
	{#if error}
		<p class="error">This book couldn't be opened.<br /><span>{error}</span></p>
	{:else if paged}
		<div class="spread">
			{#each shown as index, i (index)}
				{#if fitted[i]}
					{#if pdf}
						<PdfPage doc={pdf} {index} width={fitted[i].width} height={fitted[i].height} />
					{:else}
						<img
							src={urls.page(book.id, index)}
							alt="Page {index + 1}"
							style="width: {fitted[i].width}px; height: {fitted[i].height}px;"
							draggable="false"
						/>
					{/if}
				{/if}
			{/each}
		</div>
		<!-- click the left / right side of the page to turn it -->
		<button class="zone left" onclick={goLeft} aria-label={rtl ? 'Next page' : 'Previous page'}></button>
		<button class="zone right" onclick={goRight} aria-label={rtl ? 'Previous page' : 'Next page'}></button>
	{:else}
		<div class="epub" bind:this={epubHost}></div>
	{/if}
</main>

<div class="pager">
	<button class="turn" onclick={goLeft} aria-label={rtl ? 'Next page' : 'Previous page'}>←</button>
	<span class="label">{label}</span>
	<button class="turn" onclick={goRight} aria-label={rtl ? 'Previous page' : 'Next page'}>→</button>
</div>

<style>
	.title {
		position: fixed;
		top: 16px;
		left: 50%;
		transform: translateX(-50%);
		max-width: 60%;
		font-family: 'Kurale', serif;
		font-size: 24px;
		letter-spacing: 0.04em;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		opacity: 0.8;
	}

	.nav-back { left: calc(6.25% - 28px); top: 50%; transform: translateY(-50%); }
	.nav-full { right: calc(6.25% - 28px); top: 50%; transform: translateY(-50%); }
	.nav-manga { right: calc(6.25% - 28px); top: calc(6.25% + 8px); }

	/* the pages sit between the side buttons, under the title, above the pager */
	.stage {
		position: fixed;
		top: calc(6.25% + 24px);
		bottom: calc(6.25% + 24px);
		left: calc(6.25% + 44px);
		right: calc(6.25% + 44px);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.spread {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 0;
	}

	/* rounded page corners; in a two-page spread only the outer ones, the spine stays straight */
	.spread > :global(img),
	.spread > :global(canvas) {
		border-radius: 10px;
	}

	.spread > :global(:first-child:not(:last-child)) {
		border-top-right-radius: 0;
		border-bottom-right-radius: 0;
	}

	.spread > :global(:last-child:not(:first-child)) {
		border-top-left-radius: 0;
		border-bottom-left-radius: 0;
	}

	.spread img {
		display: block;
		object-fit: contain;
		box-shadow: 0 1px 12px rgba(0, 0, 0, 0.08);
		user-select: none;
	}

	.zone {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 35%;
		background: transparent;
		border: none;
		cursor: url('/ink/hand.svg'), pointer;
	}

	.zone.left { left: 0; }
	.zone.right { right: 0; }

	.epub {
		width: 100%;
		height: 100%;
	}

	.pager {
		position: fixed;
		left: 50%;
		bottom: calc(6.25% - 18px);
		transform: translateX(-50%);
		display: flex;
		align-items: center;
		gap: 1.5rem;
	}

	.label {
		min-width: 120px;
		text-align: center;
		font-family: 'Kurale', serif;
		font-size: 16px;
		opacity: 0.8;
	}

	.turn {
		background: transparent;
		border: none;
		font-family: 'Noto Sans JP', sans-serif;
		font-size: 26px;
		line-height: 1;
		opacity: 0.6;
		cursor: url('/ink/hand.svg'), pointer;
		transition: opacity 200ms ease;
	}

	.turn:hover { opacity: 1; }

	.error {
		font-family: 'Kurale', serif;
		font-size: 20px;
		text-align: center;
		line-height: 1.6;
	}

	.error span {
		font-size: 14px;
		opacity: 0.5;
	}
</style>
