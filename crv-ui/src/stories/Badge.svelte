<script>
	/**
	 * @typedef {Object} Props
	 * @property {'info'|'success'|'warning'|'error'} [variant]
	 * @property {string} label
	 */
	const { variant = 'info', label } = $props();
</script>

<span class={['badge', `badge--${variant}`].join(' ')}>
	{label}
</span>
