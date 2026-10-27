#!/usr/bin/env node
// Prints Markdown release notes for a tag, built from the commit subjects
// since the previous tag. Used by .github/workflows/release.yml because work
// lands on main directly, so PR-based generated notes would miss it.
//
// Usage: node scripts/release-notes.mjs <tag>
// Needs full history and tags (actions/checkout with fetch-depth: 0).

import { execFileSync } from 'node:child_process'

const tag = process.argv[2]
if (!tag) {
  console.error('Usage: node scripts/release-notes.mjs <tag>')
  process.exit(1)
}

const git = (...args) =>
  execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()

let previous = ''
try {
  previous = git('describe', '--tags', '--abbrev=0', `${tag}^`)
} catch {
  // First release: no earlier tag, so list everything up to this one.
}

const SEP = '\u001f'
const commits = git(
  'log',
  '--no-merges',
  '--reverse',
  `--format=%h${SEP}%s`,
  previous ? `${previous}..${tag}` : tag
)
  .split('\n')
  .filter(Boolean)
  .map((line) => {
    const [hash, subject] = line.split(SEP)
    return { hash, subject: subject.trim() }
  })
  // Housekeeping commits that say nothing about the app itself, including
  // reverting a version bump (`Revert "Bump version to …"`).
  .filter(({ subject }) => !/^(revert "?)?(bump version|initial commit$)/i.test(subject))

const repoUrl =
  process.env.GITHUB_SERVER_URL && process.env.GITHUB_REPOSITORY
    ? `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}`
    : ''

const lines = ['## Changes', '']
if (commits.length === 0) lines.push('- Maintenance release.')
for (const { hash, subject } of commits) {
  lines.push(
    repoUrl ? `- ${subject} ([${hash}](${repoUrl}/commit/${hash}))` : `- ${subject} (${hash})`
  )
}
if (previous && repoUrl) {
  lines.push('', `**Full changelog**: ${repoUrl}/compare/${previous}...${tag}`)
}
process.stdout.write(lines.join('\n') + '\n')
