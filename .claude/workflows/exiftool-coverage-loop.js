export const meta = {
  name: 'exiftool-coverage-loop',
  description: 'Find oxidex/ExifTool tag-coverage gaps and fix them in a forever loop, one subagent per format, until two rounds close nothing',
  phases: [
    { title: 'Find', detail: 'run tag-comparison against the ExifTool test corpus + samples' },
    { title: 'Fix', detail: 'one isolated-worktree agent per format with gaps' },
    { title: 'Merge', detail: 'sequential merge-back with a regression safety net' },
  ],
}

const CACHE_DIR = (args && args.cacheDir) || '/tmp/oxidex-exiftool-cache'

const COMPARISON_REPORT_SCHEMA = {
  type: 'object',
  properties: {
    overall_coverage: { type: 'number' },
    total_regressions: { type: 'number' },
    summary: { type: 'string' },
    by_format: {
      type: 'object',
      additionalProperties: {
        type: 'object',
        properties: {
          format: { type: 'string' },
          files_tested: { type: 'number' },
          coverage_percentage: { type: 'number' },
          total_exiftool_tags: { type: 'number' },
          missing_in_oxidex: {
            type: 'array',
            items: {
              type: 'object',
              properties: {
                name: { type: 'string' },
                family: { type: 'string' },
                value: { type: 'string' },
                tag_id: { type: ['string', 'null'] },
                source_file: { type: ['string', 'null'] },
              },
              required: ['name', 'family', 'value'],
            },
          },
          value_differences: {
            type: 'array',
            items: {
              type: 'object',
              properties: {
                tag_key: { type: 'string' },
                exiftool_value: { type: 'string' },
                oxidex_value: { type: 'string' },
                source_file: { type: 'string' },
              },
              required: ['tag_key', 'exiftool_value', 'oxidex_value', 'source_file'],
            },
          },
          regressions: { type: 'array', items: { type: 'string' } },
          // For large formats the relaying agent truncates these arrays
          // rather than writing thousands of entries into its own
          // structured-output call, and adds these markers when it does.
          // Consumers needing the complete list must re-derive it directly
          // (e.g. by re-running tag-comparison --format X themselves)
          // rather than trusting missing_in_oxidex/value_differences here
          // to be exhaustive.
          missing_in_oxidex_truncated: { type: 'boolean' },
          missing_in_oxidex_total_count: { type: 'number' },
          value_differences_truncated: { type: 'boolean' },
          value_differences_total_count: { type: 'number' },
        },
        required: ['format', 'missing_in_oxidex', 'value_differences', 'regressions'],
      },
    },
  },
  required: ['by_format'],
}

function findGapsPrompt() {
  return `Run \`EXIFTOOL_CACHE_DIR=${CACHE_DIR} just compare-exiftool-full\` from the oxidex repository root. ` +
    `This builds the tag-comparison binary, downloads or reuses a cached ExifTool release plus its t/images ` +
    `test corpus and camera sample set, and writes comparison.json in the repo root. Read comparison.json and ` +
    `return its contents as your structured output verbatim: the by_format map keyed by format name, each ` +
    `with missing_in_oxidex, value_differences, and regressions. If a format's missing_in_oxidex or ` +
    `value_differences array is large (roughly 50+ entries), truncate it to a representative sample and set ` +
    `the corresponding missing_in_oxidex_truncated / value_differences_truncated to true and ` +
    `missing_in_oxidex_total_count / value_differences_total_count to the real total count -- don't silently ` +
    `truncate without those markers, since downstream consumers rely on them to know the list isn't ` +
    `exhaustive. Do not modify or commit anything -- this is a read-only discovery step.`
}

phase('Find')
const report = await agent(findGapsPrompt(), {
  label: 'find-gaps',
  schema: COMPARISON_REPORT_SCHEMA,
})

log(`find stage: ${Object.keys(report.by_format || {}).length} formats in report`)
return report
