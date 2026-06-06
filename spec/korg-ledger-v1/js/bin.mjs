#!/usr/bin/env node
// Thin CLI entrypoint for `npx @korgg/ledger-verify`. Kept separate from
// verify.mjs so that module stays shebang-free and browser-importable.
import { cli } from "./verify.mjs";
process.exit(await cli(process.argv.slice(2)));
