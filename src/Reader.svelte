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

	// a small window shows one page at a time, staying on the same page
	let winWidth = $state(window.innerWidth);
	let winHeight = $state(window.innerHeight);
	let single = $derived(winWidth < 760 || winHeight < 560);

	$effect(() => {
		const one = single;
		untrack(() => {
			if (!sizes.length) return;
			const page = spreads[current]?.[0] ?? 0;
			spreads = buildSpreads(sizes, one);
			current = spreadOf(spreads, page);
		});
	});

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

	// preload the neighbouring comic spreads so turning the page is instant
	$effect(() => {
		if (!isComic(book.format)) return;
		for (const i of [...(spreads[current + 1] ?? []), ...(spreads[current - 1] ?? [])]) new Image().src = urls.page(book.id, i);
	});

	// ── turning the page (comics) ──
	// The new spread is shown at once; the old one lies on top of it and its outer
	// page turns over the spine, showing the new page on its back.
	type Turn = { id: number; pages: number[]; sizes: PageSize[]; dir: 'left' | 'right' };
	let turn: Turn | null = $state(null);
	let turnId = 0;

	function go(target: number) {
		if (target === current) return;
		if (isComic(book.format) && fitted.length) {
			// forward turns the page to the left, unless the book reads right to left
			const dir = target > current !== rtl ? 'left' : 'right';
			turn = { id: ++turnId, pages: shown, sizes: fitted, dir };
		}
		current = target;
	}

	let flip = $derived.by(() => {
		if (!turn) return null;
		const { pages, sizes: old, dir } = turn;
		const width = old.reduce((sum, s) => sum + s.width, 0);
		const height = Math.max(...old.map(s => s.height));
		let x = 0;
		const boxes = pages.map((index, i) => {
			const box = { index, left: x, top: (height - old[i].height) / 2, width: old[i].width, height: old[i].height };
			x += old[i].width;
			return box;
		});
		const leaf = dir === 'left' ? boxes[boxes.length - 1] : boxes[0];
		const rest = boxes.filter(b => b !== leaf);
		// two pages before and after: the back of the leaf is the new page on the side it lands
		const back = boxes.length === 2 && shown.length === 2 ? (dir === 'left' ? shown[0] : shown[1]) : null;
		return { width, height, leaf, rest, back };
	});

	function turned(id: number) {
		if (turn?.id === id) turn = null;
	}

	function next() {
		if (paged) go(Math.min(current + 1, spreads.length - 1));
		else rendition?.next();
	}

	// going back from the first page returns to the library
	function prev() {
		if (paged) {
			if (current === 0) close();
			else go(current - 1);
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
		spreads = buildSpreads(sizes, single);
		current = spreadOf(spreads, Number(book.position ?? 0) || 0);
	}

	/** The theme's ink (app.css), for the book's own pages: they live in a frame of their own. */
	const ink = () => getComputedStyle(document.documentElement).getPropertyValue('--text').trim() || '#474747';

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
				color: `${ink()} !important`,
				background: 'transparent !important',
				'line-height': '1.75 !important',
			},
			'p, li, blockquote': { 'font-family': "'Kurale', serif !important" },
			a: { color: `${ink()} !important` },
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
		// the Mac switched between light and dark while a book is open
		const scheme = matchMedia('(prefers-color-scheme: dark)');
		const recolor = () => rendition?.themes.override('color', ink(), true);
		scheme.addEventListener('change', recolor);
		return () => {
			scheme.removeEventListener('change', recolor);
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

<svelte:window bind:innerWidth={winWidth} bind:innerHeight={winHeight} onkeydown={handleKey} />

<p class="title" title={book.title}>{book.title}</p>

<button class="side boxed nav-back" onclick={close} title="Library (Esc)">BACK</button>
<button class="side boxed nav-full" onclick={toggleFullscreen} title="Full screen (F)">FULL</button>
{#if paged}
	<button class="side switch nav-manga" class:ink-mark-v={rtl} onclick={toggleManga} title="Manga: right to left (M)">MANGA</button>
	<button class="corner bottom-left" class:on={rtl} onclick={toggleManga} title="Manga: right to left (M)">MANGA</button>
{/if}

<!-- a small window: the pages take it all -->
<button class="corner top-right" onclick={close} title="Library (Esc)">BACK</button>
<button class="corner bottom-right" onclick={toggleFullscreen} title="Full screen (F)">FULL</button>

<main class="stage fade-in" bind:clientWidth={stageWidth} bind:clientHeight={stageHeight}>
	{#if error}
		<p class="error">This book couldn't be opened.<br /><span>{error}</span></p>
	{:else if paged}
		<div class="spread">
			{#each shown as index, i (index)}
				{#if fitted[i]}
					<div class="page" style="width: {fitted[i].width}px; height: {fitted[i].height}px;">
						{#if pdf}
							<PdfPage doc={pdf} {index} width={fitted[i].width} height={fitted[i].height} />
						{:else}
							<img src={urls.page(book.id, index)} alt="Page {index + 1}" draggable="false" />
						{/if}
					</div>
				{/if}
			{/each}
		</div>
		{#if turn && flip}
			{#key turn.id}
				{@const id = turn.id}
				<div class="flip {turn.dir}" class:solo={!flip.back} style="width: {flip.width}px; height: {flip.height}px;">
					{#each flip.rest as box (box.index)}
						<div class="page rest" style="left: {box.left}px; top: {box.top}px; width: {box.width}px; height: {box.height}px;">
							<img src={urls.page(book.id, box.index)} alt="" draggable="false" />
						</div>
					{/each}
					<div
						class="leaf"
						style="left: {flip.leaf.left}px; top: {flip.leaf.top}px; width: {flip.leaf.width}px; height: {flip.leaf.height}px;"
						onanimationend={() => turned(id)}
					>
						<div class="page face"><img src={urls.page(book.id, flip.leaf.index)} alt="" draggable="false" /></div>
						{#if flip.back !== null}
							<div class="page face back"><img src={urls.page(book.id, flip.back)} alt="" draggable="false" /></div>
						{/if}
					</div>
				</div>
			{/key}
		{/if}
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
	/* the title and the pager sit halfway between the pages and the window edge */
	.title {
		position: fixed;
		top: calc(3.125% + 12px);
		left: 50%;
		transform: translate(-50%, -50%);
		line-height: 32px;
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

	/* a page: rounded corners and a faint grey shadow inside along its edges */
	.page {
		position: relative;
		overflow: hidden;
		border-radius: 10px;
		box-shadow: 0 1px 12px rgba(0, 0, 0, 0.08);
	}

	.page::after {
		content: '';
		position: absolute;
		inset: 0;
		border-radius: inherit;
		box-shadow: inset 0 0 6px rgba(71, 71, 71, 0.22);
		pointer-events: none;
	}

	.page img,
	.page :global(canvas) {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: contain;
		user-select: none;
	}

	/* in a two-page spread only the outer corners are round, the spine stays straight */
	.spread > .page:first-child:not(:last-child),
	.flip.left .rest,
	.flip.right .leaf .face:not(.back),
	.flip.left .leaf .back {
		border-top-right-radius: 0;
		border-bottom-right-radius: 0;
	}

	.spread > .page:last-child:not(:first-child),
	.flip.right .rest,
	.flip.left .leaf .face:not(.back),
	.flip.right .leaf .back {
		border-top-left-radius: 0;
		border-bottom-left-radius: 0;
	}

	/* ── page turn ── */
	.flip {
		position: absolute;
		left: 50%;
		top: 50%;
		transform: translate(-50%, -50%);
		perspective: 2400px;
		pointer-events: none;
	}

	.flip .rest,
	.leaf {
		position: absolute;
	}

	/* the page under the leaf disappears once the leaf, now showing its back, covers it */
	.flip .rest {
		animation: rest-hide 520ms ease-in-out forwards;
	}

	.leaf {
		transform-style: preserve-3d;
		animation: 520ms ease-in-out forwards;
	}

	.flip.left .leaf { transform-origin: left center; animation-name: turn-left; }
	.flip.right .leaf { transform-origin: right center; animation-name: turn-right; }
	/* a single page (cover, wide page): it just swings away */
	.flip.solo.left .leaf { animation-name: swing-left; }
	.flip.solo.right .leaf { animation-name: swing-right; }

	.face {
		position: absolute;
		inset: 0;
		backface-visibility: hidden;
	}

	.face.back {
		transform: rotateY(180deg);
	}

	/* shading while the page is lifted */
	.face::before {
		content: '';
		position: absolute;
		inset: 0;
		z-index: 1;
		background: rgba(71, 71, 71, 0.18);
		opacity: 0;
		animation: lift-shade 520ms ease-in-out forwards;
		pointer-events: none;
	}

	@keyframes turn-left { from { transform: rotateY(0deg); } to { transform: rotateY(-180deg); } }
	@keyframes turn-right { from { transform: rotateY(0deg); } to { transform: rotateY(180deg); } }
	@keyframes swing-left { from { transform: rotateY(0deg); opacity: 1; } to { transform: rotateY(-90deg); opacity: 0; } }
	@keyframes swing-right { from { transform: rotateY(0deg); opacity: 1; } to { transform: rotateY(90deg); opacity: 0; } }
	@keyframes rest-hide { 0%, 49.9% { opacity: 1; } 50%, 100% { opacity: 0; } }
	@keyframes lift-shade { 0%, 100% { opacity: 0; } 50% { opacity: 1; } }

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
		bottom: calc(3.125% + 12px);
		transform: translate(-50%, 50%);
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

	/* ── a small window: the pages between a smaller title and pager ── */
	@media (max-width: 759px), (max-height: 559px) {
		.title {
			top: 22px;
			max-width: calc(100% - 200px);
			font-size: 16px;
			line-height: 24px;
		}

		.stage {
			top: 44px;
			bottom: 44px;
			left: 36px;
			right: 36px;
		}

		.pager {
			bottom: 22px;
			gap: 1rem;
		}

		.label { min-width: 80px; }

		.turn { font-size: 20px; }
	}

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
