# Publishing the npm packages

This doc covers the publish flow for `@korgg/recall-mcp` and
`@korgg/introspect-mcp`. Both packages are TypeScript ports of their
Python siblings (`adapters/recall-mcp/` and `adapters/introspect-mcp/`)
intended for `npx`-style distribution to Claude Code users.

## One-time setup

### 1. Create the `@korg` org on npm

Scoped packages (`@korg/...`) require an npm org. **Public packages
under a scope are free.**

1. Go to https://www.npmjs.com/org/create
2. Org name: `korg`
3. Visibility: public
4. After creation, add yourself (and any co-maintainers) as
   members under https://www.npmjs.com/settings/korg/members

If the `@korg` scope is taken, fall back to your personal scope
(e.g. `@new1direction`) by editing each `package.json`:

```json
"name": "@new1direction/recall-mcp"
```

### 2. Log in to npm

```bash
npm login
# follow the OTP / 2FA prompts
npm whoami        # confirm
```

## Publish flow

For each package (`recall-mcp-ts`, `introspect-mcp-ts`):

```bash
cd adapters/recall-mcp-ts

# Sanity check what would ship
npm publish --dry-run

# Run the test suite one more time
npm test

# Publish (scoped packages default to private, override with --access)
npm publish --access public
```

Same flow for `introspect-mcp-ts`.

After publishing, both packages become available via:

```bash
npx -y @korgg/recall-mcp --help
npx -y @korgg/introspect-mcp thump --list-tools
```

## What gets published

The `files` field in each `package.json` ships only:

- `dist/` — compiled JavaScript + `.d.ts` declarations
- `README.md`
- `LICENSE`

The `tests/`, `src/`, `tsconfig.json`, and `node_modules/` are NOT
included. The published tarball weighs ~11–13 kB.

## Versioning

Both packages use Semantic Versioning. Bump via npm:

```bash
npm version patch    # 0.1.0 → 0.1.1
npm version minor    # 0.1.0 → 0.2.0
npm version major    # 0.1.0 → 1.0.0
```

`npm version` creates a git tag automatically. Commit + push the
version bump + tag, then `npm publish`.

## Coordinating with the Python siblings

The TS port shares the `korg:introspect@v1` schema with the Python
reference. If the schema bumps (e.g. `@v2`), both implementations must
bump in lockstep. There's a contract test in both that asserts the
current schema ID — CI on either side will fail if drift sneaks in.

Tool name (`korg-recall-mcp.recall`), exit-code table, and Capabilities
field set are all part of the cross-language contract. The Python
reference is authoritative for any new field.

## What the npm org name unlocks

When users see `npx -y @korgg/recall-mcp` in a doc or blog post, the
`@korg` prefix immediately signals "this is part of the korg
ecosystem" — discoverable on npm via `npm search @korg`. The scope
also reserves the namespace; nobody else can publish `@korg/whatever`.

## Withdraw / unpublish

npm's 24-hour withdrawal window: `npm unpublish @korgg/recall-mcp@0.1.0`
within 24 hours of publishing. After 24 hours, npm only allows
`deprecate`, not `unpublish` (security policy). Use `deprecate` for
any breaking issue:

```bash
npm deprecate @korgg/recall-mcp@0.1.0 "Critical bug — use 0.1.1+"
```
