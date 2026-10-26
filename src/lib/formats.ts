// Opening PDF and EPUB files in the UI, plus reading their title, author and cover
// once after import (comics get theirs from Rust).
import * as pdfjs from 'pdfjs-dist';
import pdfWorker from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import ePub from 'epubjs';
import type { Book as EpubBook } from 'epubjs';
import kuraleLatin from '@fontsource/kurale/files/kurale-latin-400-normal.woff2?url';
import kuraleCyrillic from '@fontsource/kurale/files/kurale-cyrillic-400-normal.woff2?url';
import { library, urls, type Book } from './books';

pdfjs.GlobalWorkerOptions.workerSrc = pdfWorker;

async function bytes(id: number): Promise<ArrayBuffer> {
	const res = await fetch(urls.file(id));
	if (!res.ok) throw new Error(await res.text());
	return res.arrayBuffer();
}

export async function openPdf(id: number) {
	return pdfjs.getDocument({ data: new Uint8Array(await bytes(id)) }).promise;
}

export async function openEpub(id: number): Promise<EpubBook> {
	const book = ePub(await bytes(id));
	await book.ready;
	return book;
}

/** Kurale inside EPUB pages (they live in their own iframe, so the app's fonts don't reach them). */
export function epubFontCss(): string {
	const abs = (u: string) => new URL(u, location.href).href;
	return `
		@font-face { font-family: 'Kurale'; font-weight: 400; font-display: swap;
			src: url('${abs(kuraleLatin)}') format('woff2');
			unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+2000-206F, U+2074, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD; }
		@font-face { font-family: 'Kurale'; font-weight: 400; font-display: swap;
			src: url('${abs(kuraleCyrillic)}') format('woff2');
			unicode-range: U+0301, U+0400-045F, U+0490-0491, U+04B0-04B1, U+2116; }`;
}

const COVER_WIDTH = 360;

function toJpeg(canvas: HTMLCanvasElement) {
	return canvas.toDataURL('image/jpeg', 0.85);
}

async function pdfDetails(id: number) {
	const doc = await openPdf(id);
	try {
		const meta = await doc.getMetadata().catch(() => null);
		const info = (meta?.info ?? {}) as { Title?: string; Author?: string };
		const page = await doc.getPage(1);
		const base = page.getViewport({ scale: 1 });
		const viewport = page.getViewport({ scale: COVER_WIDTH / base.width });
		const canvas = document.createElement('canvas');
		canvas.width = Math.round(viewport.width);
		canvas.height = Math.round(viewport.height);
		const ctx = canvas.getContext('2d')!;
		ctx.fillStyle = '#fff';
		ctx.fillRect(0, 0, canvas.width, canvas.height);
		await page.render({ canvasContext: ctx, viewport }).promise;
		return { title: info.Title, author: info.Author, cover: toJpeg(canvas) };
	} finally {
		doc.destroy();
	}
}

function loadImage(src: string): Promise<HTMLImageElement> {
	return new Promise((resolve, reject) => {
		const img = new Image();
		img.onload = () => resolve(img);
		img.onerror = reject;
		img.src = src;
	});
}

async function epubDetails(id: number) {
	const book = await openEpub(id);
	try {
		const meta = await book.loaded.metadata;
		let cover: string | undefined;
		const coverUrl = await book.coverUrl().catch(() => null);
		if (coverUrl) {
			const img = await loadImage(coverUrl);
			const canvas = document.createElement('canvas');
			canvas.width = COVER_WIDTH;
			canvas.height = Math.round((img.naturalHeight / img.naturalWidth) * COVER_WIDTH);
			canvas.getContext('2d')!.drawImage(img, 0, 0, canvas.width, canvas.height);
			cover = toJpeg(canvas);
		}
		return { title: meta.title, author: meta.creator, cover };
	} finally {
		book.destroy();
	}
}

/** Reads title / author / cover of a newly imported PDF or EPUB and stores them. */
export async function prepare(book: Book): Promise<Book> {
	try {
		const details = book.format === 'pdf' ? await pdfDetails(book.id) : await epubDetails(book.id);
		return await library.setDetails(book.id, details);
	} catch {
		// unreadable metadata is not fatal: keep the file name as the title
		return library.setDetails(book.id, {});
	}
}
