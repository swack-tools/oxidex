---
title: GitHub Pages Setup Guide
---

# GitHub Pages Setup Guide

This guide explains how OxiDex documentation is set up and deployed to GitHub Pages.

## Overview

OxiDex uses VitePress to generate static documentation that is automatically deployed to GitHub Pages via GitHub Actions whenever changes are pushed to the main branch.

## Documentation Structure

```
docs/
├── index.md              # Home page
├── .vitepress/
│   ├── config.mts       # VitePress configuration
│   ├── theme/           # Custom theme files
│   └── dist/            # Built output (generated)
├── guide/               # User guides
├── reference/           # Reference documentation
├── architecture/        # Architecture documentation (this folder)
├── performance/         # Performance documentation
├── contributing/        # Contributing guidelines
└── tag-domains/         # Tag domain documentation
```

## VitePress Configuration

The main configuration file is at `docs/.vitepress/config.mts`. Key settings:

- **Base URL**: `/` (served from custom domain oxidex.net)
- **Output Directory**: `.vitepress/dist`
- **Clean URLs**: Enabled
- **Theme**: Dark/Light mode with GitHub color scheme
- **Search**: Local search enabled

## Deployment Process

### Automatic Deployment

1. **Push to main branch**
   ```bash
   git push origin main
   ```

2. **GitHub Actions Trigger**
   - Workflow: `deploy-docs.yml`
   - Event: Push to main branch
   - Runs: Automatically on every commit

3. **Build Process**
   - Install dependencies: `npm install`
   - Build documentation: `npm run build`
   - Output to: `docs/.vitepress/dist`

4. **Deployment**
   - Deploy to GitHub Pages
   - Site available at: https://oxidex.net/

### Manual Testing Locally

To test documentation locally before pushing:

```bash
cd docs
npm install
npm run dev
```

Then open `http://localhost:5173` in your browser.

## Building Documentation

### Build for Production

```bash
cd docs
npm run build
```

This generates the static files in `docs/.vitepress/dist/`.

### Development Server

```bash
cd docs
npm run dev
```

This starts a development server with hot-reload for rapid iteration.

## Adding New Documentation

### Create a New Page

1. Create a markdown file in the appropriate directory
2. Update the VitePress configuration to add navigation links
3. Follow the frontmatter format:

```markdown
---
title: Page Title
---

# Page Title

Content goes here...
```

### Update Navigation

Edit the VitePress configuration file (`.vitepress/config.mts`) to add your new page to:
- `nav` array - for top navigation links
- `sidebar` - for sidebar navigation under the appropriate section

Example:

```typescript
sidebar: {
  '/architecture/': [
    {
      text: 'Architecture',
      items: [
        { text: 'New Page', link: '/architecture/new-page' }
      ]
    }
  ]
}
```

## Custom Domain Setup

OxiDex uses a custom domain (oxidex.net) instead of GitHub Pages default domain (swack-tools.github.io).

### DNS Configuration

The domain is configured with GitHub Pages via CNAME record pointing to GitHub's servers.

### GitHub Pages Settings

In repository settings (Settings → Pages):
- **Source**: Deploy from a branch
- **Branch**: main
- **Folder**: / (root)
- **Custom domain**: oxidex.net

## Continuous Integration

### Related Workflows

The documentation deployment is part of the broader CI/CD pipeline:

- **CI Workflow**: Runs tests, linting, and builds
- **Deploy Docs Workflow**: Builds and deploys documentation
- **Compare ExifTool Workflow**: Generates compatibility reports

## Troubleshooting

### Workflow Failures

If the deploy workflow fails:

1. Check GitHub Actions logs (Actions tab in repository)
2. Look for build errors in the workflow output
3. Common issues:
   - VitePress config syntax errors
   - Missing markdown files referenced in config
   - Node.js version incompatibilities
   - Missing dependencies

### Documentation Not Updating

If changes don't appear on the website:

1. Verify the commit was pushed to main branch
2. Check GitHub Actions to confirm workflow ran successfully
3. Clear browser cache (Ctrl+Shift+Delete or Cmd+Shift+Delete)
4. Wait 1-2 minutes for DNS propagation

### Local Build Issues

If local build fails:

```bash
# Clear node modules and reinstall
rm -rf docs/node_modules
npm install

# Clear VitePress cache
rm -rf docs/.vitepress/cache

# Try building again
npm run build
```

## Best Practices

1. **Test locally before pushing**
   - Run `npm run dev` to verify changes
   - Check navigation and links work correctly

2. **Keep documentation up-to-date**
   - Update docs when adding new features
   - Add architecture changes to relevant docs

3. **Use clear markdown structure**
   - Use proper heading hierarchy (H1 → H2 → H3)
   - Add table of contents for long pages
   - Include code examples where helpful

4. **Review workflow logs**
   - Monitor Actions tab for build issues
   - Fix errors promptly to keep site current

## References

- [VitePress Documentation](https://vitepress.dev/)
- [GitHub Pages Documentation](https://docs.github.com/en/pages)
- [Markdown Guide](https://www.markdownguide.org/)

---

*For questions about the documentation setup, refer to the Architecture documentation or the Contributing guide.*
