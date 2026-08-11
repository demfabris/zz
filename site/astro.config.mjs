// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// GitHub Pages project site. For a custom domain later: change `site`,
// set `base` to '/', and drop the prefix from links in src/pages/index.astro.
export default defineConfig({
	site: 'https://demfabris.github.io',
	base: '/zz',
	redirects: { '/docs': '/zz/docs/getting-started' },
	integrations: [
		starlight({
			title: 'zz',
			description: 'Terminal and browser. One mux.',
			logo: {
				light: './src/assets/zz-on-light.svg',
				dark: './src/assets/zz.svg',
				replacesTitle: false,
			},
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/demfabris/zz' }],
			customCss: ['./src/styles/custom.css'],
			// Code frames get the landing page's corner and hairline. Two rules
			// here: values must be literal (Expressive Code drops a `var()` it
			// cannot parse), and `codeBackground` must be left alone — EC picks the
			// foreground by contrast against it, and a translucent grey reads as a
			// light backdrop, which turns every code block black-on-black. The
			// surface colour comes from --sl-color-gray-6/-7 in custom.css instead.
			expressiveCode: {
				styleOverrides: {
					borderRadius: '14px',
					borderColor: 'rgba(128, 128, 128, 0.26)',
				},
			},
			sidebar: [
				{ label: 'Getting started', slug: 'docs/getting-started' },
				{ label: 'tmux compatibility', slug: 'docs/tmux' },
				{ label: 'Browser panes', slug: 'docs/browser' },
				{
					label: 'Agent panes',
					slug: 'docs/agents',
					badge: { text: 'Upcoming', variant: 'caution' },
				},
				{ label: 'CLI', slug: 'docs/cli' },
				{ label: 'Configuration', slug: 'docs/configuration' },
			],
		}),
	],
});
