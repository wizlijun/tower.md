import { strict as assert } from 'node:assert'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { checkChangelogs, prepareChangelogs, releaseChangelogs } from './release-changelog.mjs'

const english = '# Changelog\n\n## Unreleased\n\n- New feature.\n\n## v0.1.0 — 2026-10-20\n\n- Original feature.\n'
const chinese = '# 更新日志\n\n## 未发布\n\n- 新功能。\n\n## v0.1.0 — 2026-10-20\n\n- 原功能。\n'
const pair = [english, chinese]

test('requires both pending sections to contain real content and be first and unique', () => {
  assert.doesNotThrow(() => checkChangelogs(pair))
  for (const index of [0, 1]) {
    for (const replacement of ['', '<!-- TODO -->\n### Added']) {
      const changed = [...pair]
      changed[index] = changed[index].replace(index === 0 ? '- New feature.' : '- 新功能。', replacement)
      assert.throws(() => checkChangelogs(changed), /不能为空/)
    }
  }
  assert.throws(() => checkChangelogs([english.replace('## Unreleased', '## Pending'), chinese]), /唯一/)
  assert.throws(() => checkChangelogs([english + '\n## Unreleased\n- Another\n', chinese]), /唯一/)
})

test('requires the complete ordered version lists to match and rejects duplicates', () => {
  assert.throws(() => checkChangelogs([english, chinese.replace('v0.1.0', 'v0.0.9')]), /逐一一致/)
  assert.throws(() => checkChangelogs([english + '\n## v0.0.9\n- Older\n', chinese]), /逐一一致/)
  assert.throws(() => checkChangelogs([english + '\n## v0.1.0\n- Duplicate\n', chinese]), /重复版本/)
  const first = english + '\n## v0.0.9\n- Older\n'
  const second = chinese.replace('v0.1.0', 'v0.0.9') + '\n## v0.1.0\n- Older\n'
  assert.throws(() => checkChangelogs([first, second]), /逐一一致/)
})

test('rejects invalid release metadata and never reuses a published version', () => {
  for (const version of ['v0.1.1', 'next', '0.1.1\n## Fake']) {
    assert.throws(() => prepareChangelogs(pair, version, '2026-10-21'), /版本应为/)
  }
  assert.throws(() => prepareChangelogs(pair, '0.1.0', '2026-10-21'), /已存在/)
  for (const date of [undefined, '2026-02-30', 'tomorrow']) {
    assert.throws(() => prepareChangelogs(pair, '0.1.1', date), /日期必须/)
  }
})

test('checks before rotating both files, preserves history, and blocks a second empty release', () => {
  const root = mkdtempSync(join(tmpdir(), 'towermd-changelog-'))
  const paths = ['CHANGELOG.md', 'CHANGELOG.zh-CN.md'].map((file) => join(root, file))
  try {
    paths.forEach((path, index) => writeFileSync(path, pair[index]))
    assert.match(releaseChangelogs(root), /门禁通过/)
    assert.match(releaseChangelogs(root, { version: '0.1.1', date: '2026-10-21' }), /未写入/)
    assert.deepEqual(paths.map((path) => readFileSync(path, 'utf8')), pair)
    const expected = prepareChangelogs(pair, '0.1.1', '2026-10-21')
    releaseChangelogs(root, { version: '0.1.1', date: '2026-10-21', write: true })
    assert.deepEqual(paths.map((path) => readFileSync(path, 'utf8')), expected)
    expected.forEach((text, index) => {
      assert.ok(text.includes(`## v0.1.1 — 2026-10-21\n\n${index === 0 ? '- New feature.' : '- 新功能。'}`))
      assert.ok(text.endsWith(pair[index].slice(pair[index].indexOf('## v0.1.0'))))
    })
    assert.throws(() => releaseChangelogs(root, { version: '0.1.2', date: '2026-10-22', write: true }), /不能为空/)
    assert.deepEqual(paths.map((path) => readFileSync(path, 'utf8')), expected)
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('a mismatch refuses preparation before either file is changed', () => {
  const root = mkdtempSync(join(tmpdir(), 'towermd-changelog-'))
  try {
    writeFileSync(join(root, 'CHANGELOG.md'), english)
    const mismatched = chinese.replace('v0.1.0', 'v0.0.9')
    writeFileSync(join(root, 'CHANGELOG.zh-CN.md'), mismatched)
    assert.throws(() => releaseChangelogs(root, { version: '0.1.1', date: '2026-10-21', write: true }), /逐一一致/)
    assert.equal(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'), english)
    assert.equal(readFileSync(join(root, 'CHANGELOG.zh-CN.md'), 'utf8'), mismatched)
  } finally { rmSync(root, { recursive: true, force: true }) }
})
