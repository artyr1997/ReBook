// Bridge to the Rust library (src-tauri/src/lib.rs) and the page-layout rules.
import { invoke } from '@tauri-apps/api/core';

export type Format = 'pdf' | 'epub' | 'cbz' | 'cbr';

export interface Book {
	id: number;
	title: string;
	author: string;
	format: Format;
	addedAt: number;
	openedAt: number | null;
	/** page index (PDF, comics) or EPUB CFI */
	position: string | null;
	/** 0 … 1 */
	progress: number;
	hasCover: boolean;
	/** title / author / cover already read from the file */
	prepared: boolean;
}

export interface PageSize {
	width: number;
	height: number;
}

export const library = {
	list: () => invoke<Book[]>('list_books'),
	import: (paths: string[]) => invoke<{ books: Book[]; errors: string[] }>('import_books', { paths }),
	setDetails: (id: number, details: { title?: string; author?: string; cover?: string }) =>
		invoke<Book>('set_details', { id, title: details.title ?? null, author: details.author ?? null, cover: details.cover ?? null }),
	setProgress: (id: number, position: string, progress: number) => invoke<void>('set_progress', { id, position, progress }),
	remove: (id: number) => invoke<void>('remove_book', { id }),
	comicPages: (id: number) => invoke<PageSize[]>('comic_pages', { id }),
};

/** Files and images are served by Rust over the rebook:// protocol. */
export const urls = {
	file: (id: number) => `rebook://localhost/file/${id}`,
	page: (id: number, index: number) => `rebook://localhost/page/${id}/${index}`,
	cover: (id: number, version = 0) => `rebook://localhost/cover/${id}?v=${version}`,
};

export const isComic = (f: Format) => f === 'cbz' || f === 'cbr';

export const formatName: Record<Format, string> = {
	pdf: 'PDF',
	epub: 'EPUB',
	cbz: 'Comic',
	cbr: 'Comic',
};

const isPortrait = (p: PageSize) => p.height > p.width;

/**
 * Groups pages into what is shown at once:
 * - portrait pages go side by side, two per screen
 * - a landscape page (wider than tall) gets the whole screen
 * - the cover stands alone, like the front of a printed comic
 * - a portrait page left without a portrait partner stands alone
 */
export function buildSpreads(pages: PageSize[]): number[][] {
	const spreads: number[][] = [];
	let i = 0;
	while (i < pages.length) {
		if (i > 0 && isPortrait(pages[i]) && i + 1 < pages.length && isPortrait(pages[i + 1])) {
			spreads.push([i, i + 1]);
			i += 2;
		} else {
			spreads.push([i]);
			i += 1;
		}
	}
	return spreads;
}

/** The spread that contains page `index`. */
export function spreadOf(spreads: number[][], index: number): number {
	const found = spreads.findIndex(s => s.includes(index));
	return found >= 0 ? found : 0;
}

/** Display sizes (CSS px) for the pages of one spread, fitted into `box` side by side. */
export function fitSpread(pages: PageSize[], box: { width: number; height: number }): PageSize[] {
	const ratios = pages.map(p => p.width / p.height);
	const totalRatio = ratios.reduce((a, b) => a + b, 0);
	const height = Math.min(box.height, box.width / totalRatio);
	return ratios.map(r => ({ width: Math.floor(height * r), height: Math.floor(height) }));
}

/** "12 / 180" or "12–13 / 180" */
export function pageLabel(spread: number[] | undefined, total: number): string {
	if (!spread || total === 0) return '';
	const first = spread[0] + 1;
	const last = spread[spread.length - 1] + 1;
	return `${first === last ? first : `${first}–${last}`} / ${total}`;
}
