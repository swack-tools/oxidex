import { h } from 'vue'
import DefaultTheme from 'vitepress/theme'
import './custom.css'

// Injected by docs/.vitepress/config.mts (vite.define). null on the stable
// channel, so the banner branch below is dead code there and the stable site
// renders exactly the default layout.
declare const __OXIDEX_PREVIEW__: {
  branch: string
  sha: string
  shortSha: string
  commitUrl: string
} | null

const preview = __OXIDEX_PREVIEW__

// Site-wide "not a release" banner for the development-preview channel. Its
// styles (and the --vp-layout-top-height offset VitePress needs) are emitted
// into <head> by config.mts, so they are present at first paint.
const PreviewBanner = () =>
  preview &&
  h('div', { class: 'oxidex-preview-banner', role: 'note' }, [
    // One inline wrapper: the banner is a flex container, and bare text
    // children of a flex container lose their edge whitespace.
    h('span', [
      `Development preview of ${preview.branch} at `,
      h('a', { href: preview.commitUrl, target: '_blank', rel: 'noreferrer' }, preview.shortSha),
      ' — not a release.'
    ])
  ])

export default {
  extends: DefaultTheme,
  ...(preview
    ? { Layout: () => h(DefaultTheme.Layout, null, { 'layout-top': PreviewBanner }) }
    : {})
}
