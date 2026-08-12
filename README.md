# Template

A battle-tested, general-purpose GitHub template repository designed to jumpstart any project—from simple Docker images and bash scripts to complex JavaFX applications and Rust terrain generators.

## ✨ Why Use This Template?

This template eliminates the repetitive setup work for new repositories by providing a solid foundation of automation, linting, and development environment configuration. Whether you're starting a solo side project or a team collaboration, this template has you covered.

### 🎯 Key Features

1. **🤖 Automated Repository Setup**
   - One-time automatic initialization on first push to `main`
   - Cleans up template-specific files
   - Syncs issue labels from the template repository
   - Generates a starter README tailored to your project

2. **🔍 Super-Linter with Auto-Fix**
   - Automated code quality checks on all pull requests
   - Auto-fixes common formatting issues (JSON, Markdown, etc.)
   - Prettier integration for consistent code style
   - Runs only on changed files (efficient CI/CD)

3. **📦 Dev Container Support**
   - Pre-configured VS Code development container
   - Alpine Linux base with bash and git
   - Consistent development environment across machines
   - Prettier extension pre-installed

4. **📋 Comprehensive Templates**
   - Bug report template
   - Feature request template
   - Pull request template with checklist
   - Branch naming validation workflow

5. **🏷️ Rich Label System**
   - Extensive label collection for issue and PR management
   - Organized categories for types, priorities, statuses, and more
   - Perfect for both team and personal project organization

## 🚀 Getting Started

### Using This Template

1. **Create a new repository from this template:**
   - Click the "Use this template" button at the top of this repository
   - Choose a name for your new repository
   - Select public or private visibility
   - Click "Create repository from template"

2. **Clone your new repository:**

   ```bash
   git clone https://github.com/YOUR-USERNAME/YOUR-REPO-NAME.git
   cd YOUR-REPO-NAME
   ```

3. **Make your first commit and push to main:**

   ```bash
   git add .
   git commit -m "Initial commit"
   git push origin main
   ```

4. **Watch the magic happen! 🎉**
   - The `repository-setup` workflow will automatically run
   - Template-specific files will be removed (including this README)
   - Labels will be synchronized from the template
   - A new project-specific README will be generated
   - Check the Actions tab to see the setup progress

### ⚠️ Important First-Time Behavior

**The automated setup workflow runs ONLY on the first push to `main` branch.** Here's what it does:

- [x] Removes `.github/workflows/repository-setup.yml` (cleanup)
- [x] Removes the template `README.md` (this file)
- [x] Deletes all default GitHub labels
- [x] Clones the label set from the template repository
- [x] Creates a new README with your repository's name and basic structure

After this first-time setup, the workflow will never run again, keeping your repository clean and ready for your project.

## 📁 What's Included

### Development Environment

- **`.devcontainer/`** - Dev container configuration
  - `Dockerfile` - Alpine Linux with bash, git, and a non-root user
  - `devcontainer.json` - Container settings and extensions

- **`.vscode/`** - VS Code editor configuration
  - `settings.json` - Formatting, tab sizes, and Prettier setup
  - `tasks.json` - Automated task to clean up old Git branches

### GitHub Automation

- **`.github/workflows/`**
  - `branch-name-validation.yml` - Enforces branch naming conventions
  - `repository-setup.yml` - One-time setup automation (auto-removed)
  - `super-linter.yml` - Linting and auto-fixing workflow

- **`.github/ISSUE_TEMPLATE/`**
  - `bug_report.md` - Structured bug reporting
  - `feature_request.md` - Feature request template

- **`.github/pull_request_template.md`** - PR template with checklist

### Configuration Files

- `.dockerignore` - Docker build exclusions
- `.gitattributes` - Git line ending configuration
- `.gitignore` - Common patterns for environments, logs, and configs

## 🔧 Customization Guide

### Branch Naming Convention

The template enforces a `category/slug` naming pattern for branches:

**Format:** `<category>/<slug>`

**Allowed categories:**

- `feature` - New features
- `bug`, `fix` - Bug fixes
- `chore` - Maintenance tasks
- `docs` - Documentation updates
- `test`, `testing` - Test additions/modifications
- `performance` - Performance improvements
- `refactor` - Code refactoring
- `ci-cd` - CI/CD changes
- `build` - Build system changes
- `release` - Release preparation
- `hotfix` - Urgent production fixes
- `experiment` - Experimental features
- `task` - General tasks
- `security` - Security fixes
- `special` - Special cases
- `status` - Status updates

**Slug rules:**

- 3-50 characters
- Lowercase letters and numbers only
- May include `-` or `_` inside (but NOT at start or end)
- Must start and end with alphanumeric characters

**Valid examples:**

```bash
feature/add-user-authentication
bug/fix-login-redirect
docs/update-api-docs
hotfix/critical-security-patch
refactor/cleanup-database-layer
```

**Invalid examples:**

```bash
Feature/AddAuth          # uppercase not allowed
feature/_private-method  # starts with underscore
add-feature              # missing category prefix
feature/x                # slug too short (< 3 chars)
```

To modify allowed categories, edit `.github/workflows/branch-name-validation.yml` and update the `PATTERN` regex.

### Super-Linter Configuration

The linter is configured in `.github/workflows/super-linter.yml`. Key settings:

```yaml
VALIDATE_ALL_CODEBASE: false # Only lint changed files
FIX_MARKDOWN: true # Auto-fix markdown
FIX_JSON: true # Auto-fix JSON
```

**Excluded files** (not linted):

- `.devcontainer/Dockerfile`
- `.github/pull_request_template.md`
- `.github/ISSUE_TEMPLATE/*.md`
- `.github/workflows/*.yml`

To customize what gets linted or fixed, modify the environment variables in the workflow file.

### Dev Container Customization

Edit `.devcontainer/Dockerfile` to add tools for your project:

```dockerfile
# Example: Add Node.js
RUN apk add --no-cache nodejs npm

# Example: Add Python
RUN apk add --no-cache python3 py3-pip
```

Update `.devcontainer/devcontainer.json` to add VS Code extensions:

```json
"extensions": [
    "esbenp.prettier-vscode",
    "ms-python.python",
    "golang.go"
]
```

### Label Customization

After the initial setup, labels are cloned from the template repository. To customize:

1. Edit labels directly in your repository's Issues → Labels section
2. Or modify the source labels in the template repository and re-clone

## 📖 Workflows Explained

### Repository Setup Workflow

**Trigger:** First push to `main` branch only (`if: github.run_number == 1`)

**Actions:**

1. Removes template-specific files
2. Clears default GitHub labels
3. Clones label set from template
4. Generates new README with repository name
5. Commits changes as user "Vianpyro"

### Branch Name Validation Workflow

**Trigger:** Push to non-main branches or pull requests

**Actions:**

1. Extracts branch name from GitHub context
2. Validates against naming convention
3. Fails with detailed error message if invalid
4. Provides examples and guidance for fixing

### Super-Linter Workflow

**Trigger:** Push to `main` or pull request to any branch

**Actions:**

1. Checks out code with full history
2. Sets up Node.js for Prettier
3. Installs and runs Prettier on all files
4. Runs Super-Linter on changed files only
5. Auto-commits fixes on PR branches (not on `main`)

## 🛠️ VS Code Tasks

The template includes a task that automatically runs when you open the folder:

**Delete Old Git Branches** - Removes local branches whose remote counterparts have been deleted. Keeps your branch list clean after merged PRs.

To disable auto-run, edit `.vscode/tasks.json` and remove the `runOptions` section.

## 💡 Usage Tips

### For Team Projects

1. **Use labels extensively** - The template provides a comprehensive label system for organizing issues and PRs
2. **Leverage PR templates** - Fill out the checklist to ensure consistent review quality
3. **Enforce branch naming** - The validation workflow helps maintain a clean branch structure
4. **Review auto-fixes** - Super-Linter will commit formatting fixes automatically on PR branches

### For Personal Projects

1. **Skip the ceremony if needed** - Templates and checklists are guides, not requirements
2. **Disable workflows you don't need** - Delete or modify workflow files in `.github/workflows/`
3. **Customize the dev container** - Add tools specific to your project type
4. **Use the label system** - Even solo projects benefit from organization

### Common Workflows

**Starting a new feature:**

```bash
git checkout -b feature/amazing-new-thing
# ... make changes ...
git push origin feature/amazing-new-thing
# Create PR on GitHub
```

**Fixing a bug:**

```bash
git checkout -b bug/fix-critical-issue
# ... fix the bug ...
git push origin bug/fix-critical-issue
# Create PR on GitHub
```

**Quick hotfix:**

```bash
git checkout -b hotfix/urgent-production-fix
# ... apply fix ...
git push origin hotfix/urgent-production-fix
# Create PR, get it merged quickly
```

## 🤝 Contributing to Your New Project

After setup, customize the contributing guidelines in your new README. Consider including:

- Code style guidelines
- Testing requirements
- PR review process
- Communication channels

## 📝 License

**TODO:** Choose a license for your project. Common options:

- **MIT** - Permissive, allows commercial use
- **Apache 2.0** - Permissive with patent grant
- **GPL v3** - Copyleft, requires derivative works to be open source
- **BSD 3-Clause** - Permissive with attribution requirement
- **Unlicense** - Public domain dedication

Add your chosen license to a `LICENSE` file and update your README.

Visit [choosealicense.com](https://choosealicense.com/) for guidance.

## 🔄 Keeping Your Template Updated

This template will continue to evolve. To pull updates into existing repositories:

1. Add the template as a remote:

   ```bash
   git remote add template https://github.com/Vianpyro/Template.git
   git fetch template
   ```

2. Cherry-pick specific updates:

   ```bash
   git cherry-pick <commit-hash>
   ```

3. Or merge changes (may require conflict resolution):
   ```bash
   git merge template/main --allow-unrelated-histories
   ```

## 🐛 Troubleshooting

### Repository setup didn't run

- Ensure you pushed to the `main` branch
- Check the Actions tab for errors
- Verify the workflow file exists before the first push

### Branch name validation failing

- Review the error message for specific guidance
- Ensure branch name matches `category/slug` format
- Check that slug is 3-50 characters and lowercase

### Super-Linter errors

- Review the linter output in the Actions tab
- Some issues may require manual fixes
- Consider excluding problematic files in the workflow config

### Dev container not working

- Ensure Docker is installed and running
- Verify the Remote-Containers extension is installed in VS Code
- Check Docker logs for build errors

## 📚 Additional Resources

- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [VS Code Dev Containers](https://code.visualstudio.com/docs/devcontainers/containers)
- [Super-Linter Documentation](https://github.com/super-linter/super-linter)
- [Prettier Documentation](https://prettier.io/docs/en/)

---

**Ready to start your next project?** Click "Use this template" and let the automation handle the boring stuff! 🚀

_This README will be automatically replaced after your first push to `main`. Make sure to customize your new README with project-specific information._
