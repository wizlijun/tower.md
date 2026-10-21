import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const files = ['CHANGELOG.md', 'CHANGELOG.zh-CN.md']
const titles = ['Unreleased', '未发布']

function inspect(text, index) {
  const sections = [...text.matchAll(/^## (.+)\r?$/gm)]
  if (sections[0]?.[1].trim() !== titles[index]
    || sections.filter((section) => section[1].trim() === titles[index]).length !== 1) {
    throw new Error(`${files[index]} 必须以唯一的 ${titles[index]} 二级节开始`)
  }
  const start = sections[0].index + sections[0][0].length
  const end = sections[1]?.index ?? text.length
  const body = text.slice(start, end).trim()
  if (!body.replace(/<!--[\s\S]*?-->/g, '').replace(/^#{1,6} .*$/gm, '').trim()) {
    throw new Error(`${files[index]} 的 ${titles[index]} 区不能为空`)
  }
  const versions = sections.slice(1).map((section) => {
    const match = section[1].match(/^v?(\d+\.\d+\.\d+(?:-[\da-zA-Z.-]+)?)(?:\s|$)/)
    if (!match) throw new Error(`${files[index]} 的版本标题无效：${section[1]}`)
    return match[1]
  })
  if (new Set(versions).size !== versions.length) throw new Error(`${files[index]} 存在重复版本`)
  return { start, end, body, versions }
}

export function checkChangelogs(texts) {
  const sections = texts.map(inspect)
  if (JSON.stringify(sections[0].versions) !== JSON.stringify(sections[1].versions)) {
    throw new Error('两份 CHANGELOG 的版本序列必须逐一一致')
  }
  return sections
}

// Check the pending changes BEFORE rotation. The new empty Unreleased section
// deliberately blocks the next release until it has its own user-facing changes.
export function prepareChangelogs(texts, version, date) {
  const sections = checkChangelogs(texts)
  if (!/^\d+\.\d+\.\d+(?:-[\da-zA-Z.-]+)?$/.test(version)) throw new Error('版本应为 0.1.1 或 0.1.1-rc.1，不带 v')
  if (sections[0].versions.includes(version)) throw new Error(`版本 ${version} 已存在，不能重复发布`)
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || Number.isNaN(Date.parse(date))
    || new Date(date).toISOString().slice(0, 10) !== date) throw new Error('日期必须是有效的 YYYY-MM-DD')
  return texts.map((text, index) => {
    const { start, end, body } = sections[index]
    return `${text.slice(0, start).trimEnd()}\n\n## v${version} — ${date}\n\n${body}\n\n${text.slice(end)}`.trimEnd() + '\n'
  })
}

export function releaseChangelogs(root, options = {}) {
  const paths = files.map((file) => join(root, file))
  const original = paths.map((path) => readFileSync(path, 'utf8'))
  checkChangelogs(original)
  if (!options.version) {
    if (options.write || options.date) throw new Error('--write / --date 必须同时提供 --version')
    return '双语 CHANGELOG 门禁通过。'
  }
  const prepared = prepareChangelogs(original, options.version, options.date)
  if (!options.write) return `双语 CHANGELOG 门禁通过；可轮转为 ${options.version}（预览，未写入）。`
  try {
    paths.forEach((path, index) => writeFileSync(path, prepared[index]))
  } catch (error) {
    // Restore only bytes this call wrote; never overwrite a concurrent edit.
    paths.forEach((path, index) => {
      if (readFileSync(path, 'utf8') === prepared[index]) writeFileSync(path, original[index])
    })
    throw error
  }
  return `双语 CHANGELOG 已轮转为 ${options.version}；版本配置、构建、验收与 tag 仍需按发行流程完成。`
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = {}
    const args = process.argv.slice(2)
    while (args.length) {
      const flag = args.shift()
      if (flag === '--write') options.write = true
      else if (flag === '--version' || flag === '--date') {
        const value = args.shift()
        if (!value || value.startsWith('--')) throw new Error(`${flag} 缺少值`)
        options[flag.slice(2)] = value
      } else throw new Error(`未知参数：${flag}`)
    }
    console.log(releaseChangelogs(dirname(dirname(fileURLToPath(import.meta.url))), options))
  } catch (error) {
    console.error(`发行已停止：${error.message}`)
    process.exitCode = 1
  }
}
