// Svelte action: lines fade to transparent near the top / bottom edge of a
// scrollable element, only on the side where content is hidden.
// Styles live in app.css (.scroll-fade).
export function scrollFade(node: HTMLElement) {
	node.classList.add('scroll-fade');

	function update() {
		node.classList.toggle('fade-top', node.scrollTop > 1);
		node.classList.toggle('fade-bottom', node.scrollTop + node.clientHeight < node.scrollHeight - 1);
	}

	const resize = new ResizeObserver(update);
	resize.observe(node);
	const mutations = new MutationObserver(update);
	mutations.observe(node, { childList: true, subtree: true, characterData: true });

	node.addEventListener('scroll', update, { passive: true });
	node.addEventListener('input', update);
	// textarea values set by bindings don't fire events
	requestAnimationFrame(update);

	return {
		destroy() {
			resize.disconnect();
			mutations.disconnect();
			node.removeEventListener('scroll', update);
			node.removeEventListener('input', update);
		},
	};
}
