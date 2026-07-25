# Manual Workflow Trigger Guide

This guide explains how to manually trigger the ExifTool tag comparison workflow.

## Overview

The tag comparison workflow normally runs automatically when:
- Code is pushed to `src/parsers/**` directory
- ExifTool version changes in the CI environment
- Weekly scheduled execution (Sundays at 02:00 UTC)

However, you can manually trigger it anytime using GitHub's workflow dispatch feature.

## Prerequisites

- GitHub CLI installed (`gh`)
- Write access to the repository
- Network connectivity to GitHub

## Steps

### Option 1: Using GitHub CLI (Recommended)

```bash
# Trigger the workflow
gh workflow run compare-exiftool.yml

# Monitor execution
gh run list --workflow compare-exiftool.yml --limit 5

# View detailed logs
gh run view <RUN_ID> --log
```

### Option 2: Using GitHub Web UI

1. Navigate to [Actions tab](https://github.com/oxidex/oxidex/actions)
2. Click "Generate Tag Comparison Report" workflow
3. Click "Run workflow" button
4. Confirm "Run workflow" in the modal
5. Monitor execution in real-time

## What Happens During Execution

### 1. Environment Setup (2-5 minutes)
- Checkout repository code
- Detect ExifTool version installed in runner
- Check cache for previously downloaded test suite

### 2. Test Suite Download (if needed, 3-10 minutes)
- If cache hit: Skip download (fast path)
- If cache miss: Download version-specific test suite
- Validate test images directory exists
- Extract images to working directory

### 3. Binary Build (3-8 minutes)
- Build Rust project in release mode
- Compile tag-comparison binary
- Link dependencies

### 4. Report Generation (5-15 minutes)
- Execute tag-comparison binary
- Process 102+ test images
- Compare OxiDex extraction vs ExifTool
- Generate JSON report with metrics
- Create markdown summary

### 5. HTML Report Generation (1-2 minutes)
- Create formatted HTML view
- Copy comparison data to deployment directory

### 6. GitHub Pages Deployment (1-2 minutes)
- Upload to GitHub Pages
- Make accessible at oxidex.net/tag-comparison/

### 7. Summary Report (1 minute)
- Display execution summary
- Report cache hit/miss status
- Show final deployment status

**Total Expected Time:**
- Cache hit (typical): 8-15 minutes
- Cache miss (first run or version change): 15-30 minutes

## Monitoring Execution

### Real-Time Monitoring

```bash
# Watch workflow in real-time
gh run watch <RUN_ID>

# Or list recent runs
gh run list --workflow compare-exiftool.yml
```

### Checking Results

```bash
# View workflow status
gh run view <RUN_ID>

# View logs for specific step
gh run view <RUN_ID> --step 'Build tag-comparison binary'

# Download full logs
gh run view <RUN_ID> --log > workflow.log
```

### Expected Output

Successful execution shows:
```
✅ ExifTool installed version: X.XX
✅ Cache configuration verified
[Cache hit status shown]
🔨 Building tag-comparison binary...
Finished release [optimized]
📊 Generating tag comparison report...
✅ Report generated successfully
🎨 Generating HTML report...
✅ HTML report generated
✅ Tag Comparison Workflow Complete
```

## Interpreting Results

### Cache Behavior

**Cache Hit** (ideal):
```
Cache Hit: true
⚡ Used cached test suite (no download needed)
Total Time: 8-15 minutes
```

**Cache Miss** (first run or version change):
```
Cache Hit: false
📥 Downloaded fresh test suite
Total Time: 15-30 minutes
```

### Report Generation Outcomes

1. **Successful** (Report Generated: true)
   - JSON and HTML reports created
   - Deployed to GitHub Pages
   - Accessible immediately at oxidex.net/tag-comparison/

2. **Warnings** (Report Generated: false)
   - Check logs for errors
   - Common causes:
     - ExifTool not available
     - Test images not found
     - Disk space issues
   - Review specific step logs

## Troubleshooting

### Workflow Fails to Start

```bash
# List available workflows
gh workflow list

# Check workflow syntax
gh workflow view compare-exiftool.yml
```

**Solution**: Ensure workflow file exists at `.github/workflows/compare-exiftool.yml`

### Cache Download Fails

**Symptom**: Workflow hangs on "Download ExifTool release" step

**Solution**:
- Check internet connectivity
- Verify ExifTool version exists
- Review logs for specific error

```bash
gh run view <RUN_ID> --log | grep -A 5 "Download ExifTool"
```

### ExifTool Not Found

**Symptom**: Error about missing exiftool binary

**Solution**: This is expected - ExifTool is pre-installed in GitHub Actions. If missing:
- Wait for runner initialization
- Check runner logs in GitHub UI

### Test Images Not Found

**Symptom**: Error "Test images not found in ExifTool"

**Solution**:
- Verify ExifTool version is valid
- Check download completed successfully
- Review logs for extraction errors

## Advanced Usage

### Trigger Multiple Workflows

```bash
# Trigger all workflow-able events
gh workflow run compare-exiftool.yml --ref main
gh workflow run tests.yml --ref main
```

### Check Workflow Configuration

```bash
# View workflow details
gh workflow view compare-exiftool.yml

# View workflow runs
gh run list --workflow compare-exiftool.yml --limit 10
```

### Parse Report Results

```bash
# Download latest report
gh run list --workflow compare-exiftool.yml --limit 1 \
  --json databaseId | jq -r '.[0].databaseId' | \
  xargs -I {} gh run download {}

# Extract metrics
jq '.summary' comparison.json
```

## Related Documentation

- [ExifTool Compatibility Report](../reference/comparison/)
- [GitHub Actions Workflow](../../.github/workflows/compare-exiftool.yml)
- [Tag Comparison Binary Documentation](../reference/api/)
- [Testing Guide](../contributing/testing/)
