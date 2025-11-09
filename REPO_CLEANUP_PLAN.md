# Repository Cleanup Plan

## Current State
- **Total .git directory**: 289 MB
- **Working tree test fixtures**: 216 MB
- **Committed build artifacts**: 431 files (65.9 MB) in `exiftool-tags-core/target/`

## Issues Found

### 1. Build Artifacts in Git History (CRITICAL) 🔴
**Problem**: 431 build artifact files (65.9 MB) were accidentally committed in Nov 5, 2025
**Root Cause**: `.gitignore` only has `/target/`, which doesn't ignore `exiftool-tags-core/target/`
**Commits**: f7ae5d1, ff786cb ("refactor: migrate to new multi-crate structure")

### 2. Excessive Test Fixtures (MEDIUM) 🟡
**Problem**: 69 unused test fixture files (~39 MB)
**Additional**: 2 large files (167 MB) should be replaced with smaller versions
**Details**: See `FIXTURE_REMOVAL_ANALYSIS.md`

### 3. LFS Bandwidth Usage (FIXED) ✅
**Problem**: CI/CD downloading 216 MB on every job
**Solution**: Already fixed in commit bb4c8b9 (disabled LFS by default in CI)

---

## Cleanup Options

### Option A: Non-Destructive (Recommended First Step)
**Impact**: Prevents future issues, no history rewrite
**Savings**: 65.9 MB from future commits

#### Steps:
1. Fix `.gitignore` to ignore all target directories
2. Remove target files from current working tree
3. Commit the removal
4. Remove unused test fixtures
5. Replace large test files with smaller versions

#### Commands:
```bash
# 1. Fix .gitignore
echo "" >> .gitignore
echo "# Ignore target in all workspace crates" >> .gitignore
echo "**/target/" >> .gitignore

# 2. Remove target files from git
git rm -r --cached exiftool-tags-core/target/
git status  # Verify 431 deletions

# 3. Commit
git commit -m "fix(gitignore): remove build artifacts from git

- Add **/target/ to .gitignore to catch workspace crate targets
- Remove 431 build artifact files (65.9 MB) from exiftool-tags-core/target/
- Previous commits accidentally included debug builds from refactoring

This fixes the .gitignore pattern /target/ which only caught root-level,
not workspace crate targets like exiftool-tags-core/target/.

Resolves repository bloat issue."

# 4. Push
git push origin main
```

**After this**: The files are removed from HEAD, but still exist in git history (commits f7ae5d1, ff786cb)

---

### Option B: Destructive (Rewrite History)
**Impact**: Removes build artifacts from entire git history
**Savings**: ~65.9 MB from .git directory
**Risk**: ⚠️ Requires force-push, breaks existing clones

**ONLY do this if**:
- This is a personal repo OR you can coordinate with all contributors
- No open PRs that would break
- You're willing to force-push and have everyone re-clone

#### Method 1: Using git-filter-repo (Recommended)
```bash
# Install git-filter-repo
pip3 install git-filter-repo

# Backup first!
cd ..
git clone exiftool-rs exiftool-rs-backup

cd exiftool-rs

# Remove all target/ directories from history
git filter-repo --path exiftool-tags-core/target --invert-paths

# Force push
git push origin --force --all
git push origin --force --tags

# Clean up local LFS
git lfs prune
```

#### Method 2: Using BFG Repo-Cleaner
```bash
# Download BFG
# https://rtyley.github.io/bfg-repo-cleaner/

# Clone a fresh bare copy
git clone --mirror git@github.com:swack-tools/exiftool-rs.git exiftool-rs-cleanup.git

# Run BFG to remove target directories
java -jar bfg.jar --delete-folders target exiftool-rs-cleanup.git

# Clean up
cd exiftool-rs-cleanup.git
git reflog expire --expire=now --all
git gc --prune=now --aggressive

# Push
git push --force

# Re-clone your working copy
cd ..
rm -rf exiftool-rs
git clone git@github.com:swack-tools/exiftool-rs.git
cd exiftool-rs
```

---

### Option C: Hybrid Approach (Recommended)
**Step 1**: Do Option A (non-destructive) immediately
**Step 2**: Schedule Option B for later when convenient

**Rationale**:
- Option A prevents the problem from getting worse NOW
- Option A reduces future clone sizes by 65.9 MB
- Option B can be done later when safe (no active PRs, coordinated with team)

---

## Expected Savings

### Option A (Non-Destructive)
```
Current .git:             289 MB
After removing fixtures:  250 MB (-39 MB)
After replacing large:     91 MB (-159 MB more)
--------------------------------
Total savings:            -198 MB (68% reduction)
Final .git size:          ~91 MB
```

Note: Build artifacts (65.9 MB) remain in history until Option B

### Option B (Destructive - Remove from history)
```
Option A result:           91 MB
Remove build history:     ~25 MB (-65.9 MB cleanup)
--------------------------------
Final .git size:          ~25 MB (91% total reduction)
```

---

## Immediate Action Plan

### Step 1: Fix .gitignore (5 minutes)
```bash
# Add to .gitignore
echo "" >> .gitignore
echo "# Ignore target in all workspace crates" >> .gitignore
echo "**/target/" >> .gitignore

# Test it works
touch exiftool-tags-core/target/test.txt
git status  # Should not show test.txt
rm exiftool-tags-core/target/test.txt
```

### Step 2: Remove Build Artifacts from Current Commit (5 minutes)
```bash
# Remove from git index (keeps local files)
git rm -r --cached exiftool-tags-core/target/

# Check what will be removed
git status | grep deleted | wc -l  # Should show 431

# Commit
git commit -m "fix(gitignore): remove build artifacts from git

- Add **/target/ to .gitignore to catch workspace crate targets
- Remove 431 build artifact files (65.9 MB) from exiftool-tags-core/target/
- Previous commits accidentally included debug builds from refactoring

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"

# Push
git push origin main
```

### Step 3: Verify .gitignore Works (2 minutes)
```bash
# Build something
cd exiftool-tags-core
cargo build

# Verify target/ is ignored
git status  # Should show clean working tree

cd ..
```

### Step 4: Remove Unused Test Fixtures (Optional, 10 minutes)
See `FIXTURE_REMOVAL_ANALYSIS.md` for detailed commands.

```bash
# Conservative removal (skip TIFF simple files)
# Removes: duplicates, unused JPEG/PNG/PDF/MP4, TIFF complex
# Saves: ~11.5 MB

# Run removal script from FIXTURE_REMOVAL_ANALYSIS.md
```

### Step 5: Replace Large Test Files (Optional, needs file generation)
```bash
# Replace very_large.tif (137 MB → 5 MB) = save 132 MB
# Replace large_plasma.png (30 MB → 3 MB) = save 27 MB
# Total: save 159 MB

# This requires generating smaller replacement files
# Can be done separately
```

---

## Post-Cleanup Verification

After Option A (non-destructive):
```bash
# Verify target is ignored
git status  # Should be clean

# Verify build artifacts removed from HEAD
git ls-tree -r HEAD | grep "target/" | wc -l  # Should be 0

# Check .git size
du -sh .git

# Verify tests still pass
cargo test --all-features
cargo bench --no-run
```

After Option B (destructive):
```bash
# Verify history is clean
git log --all --pretty=format: --name-only | grep "target/" | wc -l  # Should be 0

# Check .git size
du -sh .git

# Verify git history
git log --oneline | head -10
```

---

## Recommendation

**Do Option A NOW** (non-destructive):
1. Fix .gitignore (add `**/target/`)
2. Remove build artifacts from current commit
3. Push changes

**Consider Option B LATER** (rewrite history):
- Wait until no active PRs
- Coordinate with any other developers
- Choose a maintenance window
- Use git-filter-repo (cleaner than BFG)

**Total time**: 10-15 minutes for Option A
**Risk**: Low (non-destructive, can be undone with git revert)
