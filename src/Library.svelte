<script lang="ts">
	// The library in the ReNote style: titles on the left (selected one inked),
	// the cover and details of the selected book on the right.
	import { onMount, tick } from 'svelte';
	import { open } from '@tauri-apps/plugin-dialog';
	import { getCurrentWebview } from '@tauri-apps/api/webview';
	import Popup from './lib/Popup.svelte';
	import { scrollFade } from './lib/scrollFade';
	import { prepare } from './lib/formats';
	import { library, urls, formatName, type Book } from './lib/books';

	let { onread, selectId = null }: { onread: (book: Book) => void; selectId?: number | null } = $props();

	let books: Book[] = $state([]);
	let loaded = $state(false);
	let selected = $state(0);
	let query = $state('');
	let popup: 'none' | 'delete' | 'info' | 'errors' = $state('none');
	let importErrors: string[] = $state([]);
	let dragging = $state(false);
	let busy = $state(false);
	// covers change after preparing; bump to reload the image
	let coverVersion = $state(0);

	let searchInput: HTMLInputElement | undefined = $state();
	let list: HTMLDivElement | undefined = $state();

	let visible = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return q
			? books.filter(b => b.title.toLowerCase().includes(q) || b.author.toLowerCase().includes(q))
			: books;
	});
	let current: Book | undefined = $derived(visible[selected]);

	function progressText(b: Book) {
		if (b.openedAt === null) return 'New';
		if (b.progress >= 0.999) return 'Finished';
		return `${Math.round(b.progress * 100)}%`;
	}

	async function load(selectBookId?: number | null) {
		books = await library.list();
		loaded = true;
		const idx = selectBookId != null ? visible.findIndex(b => b.id === selectBookId) : -1;
		selected = idx >= 0 ? idx : Math.min(selected, Math.max(visible.length - 1, 0));
		await scrollSelectedIntoView();
		prepareAll();
	}

	/** Reads title / author / cover of new PDFs and EPUBs, one at a time, in the background. */
	let preparing = false;
	async function prepareAll() {
		if (preparing) return;
		preparing = true;
		try {
			for (const b of books.filter(b => !b.prepared && (b.format === 'pdf' || b.format === 'epub'))) {
				const updated = await prepare(b);
				books = books.map(x => (x.id === updated.id ? updated : x));
				coverVersion++;
			}
		} finally {
			preparing = false;
		}
	}

	async function addBooks(paths: string[]) {
		if (!paths.length) return;
		busy = true;
		try {
			const result = await library.import(paths);
			importErrors = result.errors;
			if (result.errors.length) popup = 'errors';
			query = '';
			await load(result.books[0]?.id ?? current?.id);
		} finally {
			busy = false;
		}
	}

	async function pickFiles() {
		const picked = await open({
			multiple: true,
			filters: [{ name: 'Books', extensions: ['pdf', 'epub', 'cbz', 'cbr'] }],
		});
		if (picked) addBooks(Array.isArray(picked) ? picked : [picked]);
	}

	async function removeCurrent() {
		if (!current) return;
		await library.remove(current.id);
		popup = 'none';
		await load();
	}

	function read(book: Book | undefined = current) {
		if (book) onread(book);
	}

	async function move(delta: number) {
		if (!visible.length) return;
		selected = (selected + delta + visible.length) % visible.length;
		await scrollSelectedIntoView();
	}

	async function scrollSelectedIntoView() {
		await tick();
		list?.querySelector('.row.active')?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
	}

	function handleKeydown(e: KeyboardEvent) {
		if (popup !== 'none') {
			if (e.key === 'Escape') {
				e.preventDefault();
				popup = 'none';
			} else if (e.key === 'Enter') {
				e.preventDefault();
				if (popup === 'delete') removeCurrent();
				else popup = 'none';
			}
			return;
		}
		const mod = e.metaKey || e.ctrlKey;
		if (e.target === searchInput) {
			if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
				e.preventDefault();
				move(e.key === 'ArrowDown' ? 1 : -1);
			} else if (e.key === 'Enter') {
				e.preventDefault();
				read();
			} else if (e.key === 'Escape') {
				query = '';
				searchInput?.blur();
			}
			return;
		}
		if ((e.target as HTMLElement)?.tagName === 'INPUT') return;

		if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			e.preventDefault();
			move(e.key === 'ArrowDown' ? 1 : -1);
		} else if (e.key === 'Enter' || e.key === 'ArrowRight') {
			e.preventDefault();
			read();
		} else if (e.key === 'Backspace' || e.key === 'Delete') {
			e.preventDefault();
			if (current) popup = 'delete';
		} else if (e.code === 'Slash') {
			e.preventDefault();
			searchInput?.focus();
		} else if (!mod && e.code === 'KeyA') {
			pickFiles();
		} else if (!mod && e.code === 'KeyI') {
			popup = 'info';
		}
	}

	onMount(() => {
		load(selectId);
		// drop files onto the window to add them
		let unlisten: (() => void) | undefined;
		getCurrentWebview()
			.onDragDropEvent(event => {
				const p = event.payload;
				if (p.type === 'enter' || p.type === 'over') dragging = true;
				else if (p.type === 'leave') dragging = false;
				else if (p.type === 'drop') {
					dragging = false;
					addBooks(p.paths);
				}
			})
			.then(fn => (unlisten = fn));
		return () => unlisten?.();
	});

	const shortcuts = [
		['↑ / ↓', 'Select'],
		['Enter / →', 'Read'],
		['← → / Space', 'Turn pages'],
		['F', 'Full screen'],
		['M', 'Manga (right to left)'],
		['Esc', 'Back to library'],
		['/', 'Search'],
		['A', 'Add books'],
		['I', 'Info'],
		['Backspace', 'Remove from library'],
	];
</script>

<svelte:window
	onkeydown={handleKeydown}
	oncontextmenu={e => {
		if ((e.target as HTMLElement)?.tagName !== 'INPUT') e.preventDefault();
	}}
/>

<h1 class="title">Library</h1>

<button class="side boxed nav-read" onclick={() => read()} disabled={!current} title="Read (Enter)">READ</button>
<button class="side boxed nav-add" onclick={pickFiles} title="Add books (A)">ADD</button>
<button class="side plain nav-info" onclick={() => (popup = 'info')} title="Info (I)">INFO</button>
<button class="side plain nav-del" onclick={() => (popup = 'delete')} disabled={!current} title="Remove (Backspace)">DEL</button>

<main class="library fade-in">
	{#if loaded && books.length === 0}
		<p class="welcome">Drop PDF, EPUB or comics here<br />or press <span class="ink-mark">ADD</span>.</p>
	{:else}
		<div class="books scroll-area" bind:this={list} use:scrollFade>
			{#each visible as book, i (book.id)}
				<button
					class="row"
					class:active={i === selected}
					onclick={() => (selected = i)}
					ondblclick={() => read(book)}
				>
					<span class="name" class:ink-mark={i === selected}>{book.title}</span>
				</button>
			{:else}
				{#if loaded}<p class="empty">Nothing found.</p>{/if}
			{/each}
		</div>

		{#if current}
			{#key current.id}
				<section class="details fade-in">
					{#if current.hasCover}
						<img class="cover" src={urls.cover(current.id, coverVersion)} alt="" draggable="false" />
					{:else}
						<div class="cover placeholder"><span>{current.title}</span></div>
					{/if}
					<p class="author">{current.author || ' '}</p>
					<p class="meta">{formatName[current.format]} · {progressText(current)}</p>
				</section>
			{/key}
		{/if}
	{/if}

	<input
		bind:this={searchInput}
		bind:value={query}
		class="search"
		type="text"
		placeholder="Search"
		title="Search titles and authors (/)"
		oninput={() => (selected = 0)}
	/>
</main>

{#if busy}
	<div class="toast"><span>ADDING</span></div>
{/if}

{#if dragging}
	<div class="drop" aria-hidden="true"><p><span class="ink-mark">Drop to add</span></p></div>
{/if}

{#if popup === 'delete' && current}
	<Popup onclose={() => (popup = 'none')}>
		<p class="pop-text">Remove “{current.title}” from the library?</p>
		<p class="pop-note">The stored copy goes to the Trash.</p>
		<div class="pop-actions">
			<button class="pop-btn" onclick={removeCurrent}>YES</button>
			<button class="pop-btn" onclick={() => (popup = 'none')}>NO</button>
		</div>
	</Popup>
{:else if popup === 'errors'}
	<Popup onclose={() => (popup = 'none')}>
		<p class="pop-heading">NOT ADDED</p>
		<div class="pop-divider"></div>
		{#each importErrors as line}
			<p class="pop-note">{line}</p>
		{/each}
		<button class="pop-btn" onclick={() => (popup = 'none')}>OK</button>
	</Popup>
{:else if popup === 'info'}
	<Popup onclose={() => (popup = 'none')}>
		<p class="pop-heading">RE:BOOK</p>
		<div class="pop-divider"></div>
		<p class="pop-text">{books.length} {books.length === 1 ? 'book' : 'books'}</p>
		<div class="pop-divider"></div>
		<div class="shortcuts">
			{#each shortcuts as [key, desc]}
				<span class="key">{key}</span><span class="desc">{desc}</span>
			{/each}
		</div>
		<div class="pop-divider"></div>
		<p class="pop-footer">PDF · EPUB · CBZ · CBR</p>
	</Popup>
{/if}

<style>
	.library {
		position: fixed;
		inset: 0;
	}

	.title {
		position: fixed;
		top: calc(var(--edge-y) - 2px);
		left: 50%;
		transform: translateX(-50%);
		font-family: 'Kurale', serif;
		font-weight: 400;
		font-size: 24px;
		line-height: 32px;
		letter-spacing: 0.04em;
	}

	.nav-read { left: var(--edge-x); top: 50%; transform: translateY(-50%); }
	.nav-add { right: var(--edge-x); top: 50%; transform: translateY(-50%); }
	.nav-info { left: var(--edge-x); bottom: var(--edge-y); width: 42px; }
	.nav-del { right: var(--edge-x); bottom: var(--edge-y); width: 42px; }
	.side:disabled { opacity: 0.3; pointer-events: none; }

	/* ── list of titles ── */
	.books {
		position: absolute;
		top: calc(var(--edge-y) + 132px);
		bottom: calc(var(--edge-y) + 72px);
		left: calc(21.3% - 0.6em);
		width: 30%;
		overflow-y: auto;
		overflow-x: hidden;
		display: flex;
		flex-direction: column;
		gap: 22px;
		/* room for the ink stroke of the selected row, which the scroll box would clip */
		padding: 0.4em 0.6em;
	}

	.row {
		display: flex;
		min-width: 0;
		flex-shrink: 0;
		background: transparent;
		border: none;
		padding: 0;
		text-align: left;
		font-family: 'Kurale', serif;
		font-size: 16px;
		line-height: 1.2;
		cursor: url('/ink/hand.svg'), pointer;
		scroll-margin: 56px 0;
	}

	.name {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
		max-width: 100%;
	}

	.empty {
		font-family: 'Kurale', serif;
		font-size: 16px;
		opacity: 0.45;
	}

	/* ── selected book ── */
	.details {
		position: absolute;
		top: calc(var(--edge-y) + 132px);
		bottom: calc(var(--edge-y) + 72px);
		left: 58%;
		right: calc(var(--edge-x) + 88px);
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 14px;
	}

	.cover {
		max-width: 100%;
		max-height: calc(100% - 80px);
		object-fit: contain;
		box-shadow: 0 2px 18px rgba(0, 0, 0, 0.12);
	}

	.cover.placeholder {
		width: min(260px, 100%);
		aspect-ratio: 2 / 3;
		border: 2px solid var(--line);
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 1.5rem;
		box-shadow: none;
	}

	.cover.placeholder span {
		font-family: 'Kurale', serif;
		font-size: 22px;
		text-align: center;
		overflow-wrap: anywhere;
	}

	.author {
		font-family: 'Kurale', serif;
		font-size: 16px;
		text-align: center;
	}

	.meta {
		font-family: 'Noto Sans JP', sans-serif;
		font-size: 12px;
		letter-spacing: 0.3em;
		text-transform: uppercase;
		opacity: 0.6;
	}

	.welcome {
		position: absolute;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
		font-family: 'Kurale', serif;
		font-size: 24px;
		line-height: 2;
		text-align: center;
	}

	/* ── search ── */
	.search {
		position: absolute;
		left: 50%;
		bottom: calc(var(--edge-y) - 6px);
		transform: translateX(-50%);
		width: 360px;
		background: transparent;
		border: none;
		outline: none;
		font-family: 'Kurale', serif;
		font-size: 24px;
		line-height: 32px;
		text-align: center;
		color: var(--text);
	}

	.search::placeholder {
		color: var(--muted);
		opacity: 0.55;
	}

	/* ── drop overlay ── */
	.drop {
		position: fixed;
		inset: 0;
		z-index: 60;
		backdrop-filter: blur(6px);
		-webkit-backdrop-filter: blur(6px);
		display: flex;
		align-items: center;
		justify-content: center;
		font-family: 'Kurale', serif;
		font-size: 30px;
	}

	/* ── toast (like ReNote's SAVED) ── */
	.toast {
		position: fixed;
		top: 0;
		left: 50%;
		transform: translateX(-50%);
		width: 42px;
		padding: 1.5rem 0 calc(1.5rem - 0.6em);
		border: 2px solid var(--line);
		border-top: none;
		background: var(--bg) url('/noise.svg') repeat;
		display: flex;
		justify-content: center;
		z-index: 80;
	}

	.toast span {
		writing-mode: vertical-rl;
		text-orientation: upright;
		letter-spacing: 0.6em;
		font-size: 16px;
		line-height: 32px;
	}

	/* ── popups ── */
	.pop-note {
		font-family: 'Kurale', serif;
		font-size: 15px;
		opacity: 0.65;
		text-align: center;
		overflow-wrap: anywhere;
	}

	.shortcuts {
		display: grid;
		grid-template-columns: auto auto;
		gap: 8px 24px;
		font-size: 13px;
	}

	.shortcuts .key {
		font-family: 'Noto Sans JP', sans-serif;
		letter-spacing: 0.1em;
		text-align: right;
		opacity: 0.85;
	}

	.shortcuts .desc {
		font-family: 'Kurale', serif;
		opacity: 0.7;
	}

	.pop-footer {
		font-family: 'Kurale', serif;
		font-size: 13px;
		letter-spacing: 0.15em;
		opacity: 0.55;
	}
</style>
