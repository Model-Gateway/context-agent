// Test entry point.
//
// `npm test` runs `node --test src/__tests__/`. On Node >= 21 the positional
// argument is a glob pattern, and a bare directory matches itself rather than
// the files inside it, so Node resolves the directory to this index file.
// Import every *.test.mjs here so the whole suite runs from that one command.
import "./format.test.mjs";
