/**
 * A markdown document covering every construct the parser, the inline
 * renderer, the highlighter and the file references handle, for a
 * self-check page or a preview: `<MarkdownView markdown={sampleMarkdown}
 * files={sampleFileReferenceContext(perform)} />`. It extends the preview
 * at the bottom of MarkdownView.swift. `checkSampleMarkdown()` is the
 * test-free self-check: it parses the sample and returns what did not come
 * out as expected (an empty array when everything did).
 */
import { applyFileReferences, type FileReference, type FileReferenceContext, type PathAction } from './fileReferences'
import { highlightLine } from './highlighter'
import { parseInline, plainText, type InlineNode } from './inline'
import { parseMarkdown, type MarkdownBlock } from './parser'

export { parseMarkdown }

export const sampleMarkdown = `# Summary

The parser handles **bold**, *italic*, ***both***, ~~struck~~, \`inline code\` and [links](https://example.com "Example").
A second line in the same paragraph, with an autolink <https://example.org/docs> and a bare one https://example.net/path?q=1.
Escapes stay literal: \\*not emphasis\\*, 5 \\* 3, and entities decode: &amp; &lt;tag&gt; &copy; &#8212; &#x2192;.
A hard break follows this line\\
and this is the next line. Snake_case_words keep their underscores.

Setext heading
==============

Another setext
--------------

## Changes

### Files touched ###

#### Details

- Edited \`src/app.rs\` and src/markdown/parser.ts:42, see [the parser](src/markdown/parser.ts#L10)
  - nested item with \`code\`
  - another
    1. deep ordered
    2. items
- Ran the tests
  which continue on the next line
- Windows paths work too: \`src\\markdown\\inline.ts\`, and a bare index.html
- Mail <dev@example.com>, an image ![logo](https://example.com/logo.png)
+ plus marker
* star marker

3. Third
4. Fourth
   - sub bullet

   A paragraph inside the fourth item.

   \`\`\`sh
   $ cargo build --release -p ctx-cli
   Compiling ctx-cli v0.1.0
   $ ./target/release/ctx serve --mock
   \`\`\`

> A quoted note with **emphasis** and \`code\`.
> - a list in a quote
>
> > nested quote

\`\`\`rust
fn main() {
    let answer: u32 = 42; // the answer
    println!("{}", answer);
}
\`\`\`

\`\`\`json
{"taskId": "11", "count": 3, "ok": true, "none": null}
\`\`\`

\`\`\`typescript
interface Point { readonly x: number; y: number } /* 2-D */
const label = \`(\${p.x}, \${p.y})\`;
\`\`\`

\`\`\`python
def double(x):  # twice
    return x * 2
\`\`\`

~~~
plain text in a tilde fence
without a language
~~~

| File | Status | Note |
|---|:---:|---|
| \`src/app.rs\` | modified | a \\| literal pipe |
| \`README.md\` | added |
| extra | cells | are | dropped |

---

***

___

\`\`\`diff
--- a/src/app.rs
+++ b/src/app.rs
@@ -1,3 +1,4 @@
-let x = 1;
+let x = 2;
\`\`\`

\`\`\`json
{"unterminated": true
`

/** Changed paths of the sample project; `sampleMarkdown` references each of them. */
export const sampleChangedPaths: readonly string[] = [
  'src/app.rs',
  'src/markdown/parser.ts',
  'src/markdown/inline.ts',
  'web/index.html',
]

/** A `FileReferenceContext` for the sample: files under `sampleChangedPaths` resolve. */
export function sampleFileReferenceContext(
  perform: (reference: FileReference, action: PathAction) => void = () => {},
): FileReferenceContext {
  const root = '/tmp/sample-project'
  return {
    root,
    changedPaths: () => sampleChangedPaths,
    isFile: (path) => sampleChangedPaths.some((changed) => path === `${root}/${changed}`),
    perform,
  }
}

/** The block kinds `parseMarkdown(sampleMarkdown)` yields, in order. */
const expectedBlockKinds: readonly MarkdownBlock['kind'][] = [
  'heading', 'paragraph', 'heading', 'heading', 'heading', 'heading', 'heading', 'bulletList', 'orderedList',
  'blockquote', 'codeFence', 'codeFence', 'codeFence', 'codeFence', 'codeFence', 'table', 'horizontalRule',
  'horizontalRule', 'horizontalRule', 'codeFence', 'codeFence',
]

/**
 * The self-check: parses `sampleMarkdown` (blocks, inline markup, file
 * references, highlighting) and returns one line per expectation that
 * failed. Empty means the port behaves like the Swift parser on every
 * construct.
 */
export function checkSampleMarkdown(): string[] {
  const failures: string[] = []
  const expect = (ok: boolean, what: string) => {
    if (!ok) failures.push(what)
  }
  const blocks = parseMarkdown(sampleMarkdown)
  const kinds = blocks.map((block) => block.kind)
  expect(kinds.join(',') === expectedBlockKinds.join(','), `block kinds: ${kinds.join(',')}`)

  const headings = blocks.filter((block) => block.kind === 'heading')
  expect(
    headings.map((block) => `${block.level}:${block.text}`).join('|') ===
      '1:Summary|1:Setext heading|2:Another setext|2:Changes|3:Files touched|4:Details',
    'heading levels and closing hashes',
  )

  const bullets = blocks.find((block) => block.kind === 'bulletList')
  const firstItem = bullets?.items[0]
  const nestedList = firstItem?.nested[0]
  expect(bullets?.items.length === 6, 'bullet items (-, + and * markers)')
  expect(nestedList?.kind === 'bulletList' && nestedList.items.length === 2, 'nested bullet list')
  const deep = nestedList?.kind === 'bulletList' ? nestedList.items[1]?.nested[0] : undefined
  expect(deep?.kind === 'orderedList' && deep.items.length === 2, 'ordered list nested two levels deep')

  const ordered = blocks.find((block) => block.kind === 'orderedList')
  expect(ordered?.start === 3, 'ordered list start number')
  const fourth = ordered?.items[1]?.nested.map((block) => block.kind).join(',')
  expect(fourth === 'bulletList,paragraph,codeFence', `blocks inside a list item: ${fourth ?? 'none'}`)

  const quote = blocks.find((block) => block.kind === 'blockquote')
  expect(quote?.blocks.map((block) => block.kind).join(',') === 'paragraph,bulletList,blockquote', 'quote contents')

  const fences = blocks.filter((block) => block.kind === 'codeFence')
  expect(
    fences.map((block) => block.language ?? '-').join(',') === 'rust,json,typescript,python,-,diff,json',
    'fence languages',
  )
  expect(fences[fences.length - 1]?.code.startsWith('{"unterminated": true') === true, 'unterminated fence')

  const table = blocks.find((block) => block.kind === 'table')
  expect(table?.headers.join('|') === 'File|Status|Note', 'table headers')
  expect(table?.rows.every((row) => row.length === 3) === true, 'table rows padded and truncated to the headers')
  expect(table?.rows[0]?.[2] === 'a | literal pipe', 'escaped pipe in a cell')

  const paragraph = blocks.find((block) => block.kind === 'paragraph')
  const inline = paragraph ? parseInline(paragraph.text) : []
  const kindsInline = new Set(flatten(inline).map((node) => node.kind))
  for (const kind of ['strong', 'emphasis', 'strikethrough', 'code', 'link', 'lineBreak'] as const) {
    expect(kindsInline.has(kind), `inline ${kind}`)
  }
  const links = flatten(inline).filter((node) => node.kind === 'link')
  expect(links.filter((node) => node.autolink).length === 2, 'autolinks (<…> and bare)')
  const text = plainText(inline)
  expect(text.includes('Escapes stay literal: *not emphasis*, 5 * 3,'), 'backslash escapes')
  expect(text.includes('decode: & <tag> © — →.'), 'entities')

  const files = sampleFileReferenceContext()
  const item = firstItem ? applyFileReferences(parseInline(firstItem.text), files) : []
  const chips = flatten(item).filter((node) => node.kind === 'fileReference')
  expect(
    chips.map((node) => `${node.path}${node.line !== undefined ? ':' + String(node.line) : ''}`).join(',') ===
      'src/app.rs,src/markdown/parser.ts:42,src/markdown/parser.ts:10',
    `file references: ${chips.map((node) => node.path).join(',')}`,
  )
  const windows = bullets?.items[2]
  const windowsChips = windows ? flatten(applyFileReferences(parseInline(windows.text), files)) : []
  expect(
    windowsChips.filter((node) => node.kind === 'fileReference').map((node) => node.path).join(',') ===
      'src/markdown/inline.ts,web/index.html',
    'backslash paths and bare names matching one changed path',
  )

  const json = highlightLine('{"ok": true, "n": 3}', 'json').map((span) => span.kind ?? '-')
  expect(json.join(',') === '-,key,-,keyword,-,key,-,number,-', `json highlighting: ${json.join(',')}`)
  return failures
}

function flatten(nodes: readonly InlineNode[]): InlineNode[] {
  const out: InlineNode[] = []
  for (const node of nodes) {
    out.push(node)
    if ('children' in node) out.push(...flatten(node.children))
  }
  return out
}
