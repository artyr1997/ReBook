<script lang="ts">
	import WindowControls from './lib/WindowControls.svelte';
	import Library from './Library.svelte';
	import Reader from './Reader.svelte';
	import type { Book } from './lib/books';

	let reading: Book | null = $state(null);
	// the book to keep selected when coming back from reading
	let lastRead: number | null = $state(null);
</script>

<WindowControls />

{#if reading}
	<Reader
		book={reading}
		onclose={() => {
			lastRead = reading?.id ?? null;
			reading = null;
		}}
	/>
{:else}
	<Library onread={book => (reading = book)} selectId={lastRead} />
{/if}
