import { describe, expect, it } from 'vitest'
import { rfc3339Local } from './delegation'
import { parseQuickCommand, parseUntil, QUICK_COMMANDS } from './quick-commands'

const now = new Date(2026, 9, 24, 14, 30, 15)
const global = { kind: 'prompt' as const, scope: { kind: 'global' as const } }
const task = { kind: 'prompt' as const, scope: { kind: 'task' as const, task: 'T-1' } }
const module = { kind: 'prompt' as const, scope: { kind: 'module' as const, module: 'A' } }

describe('quick commands', () => {
  it('leaves plain text and escaped slashes as messages', () => {
    expect(parseQuickCommand('看一下边界情况', task, now)).toEqual({ kind: 'message', text: '看一下边界情况' })
    expect(parseQuickCommand('/usr/bin 下的脚本别动', task, now)).toEqual({ kind: 'message', text: '/usr/bin 下的脚本别动' })
    expect(parseQuickCommand('//pause 是什么意思？', task, now)).toEqual({ kind: 'message', text: '/pause 是什么意思？' })
    expect(parseQuickCommand('/paus', global, now)).toMatchObject({ kind: 'error', message: expect.stringContaining('//') })
  })

  it('pauses and resumes the whole project or one task, never a module', () => {
    expect(parseQuickCommand('/pause', global, now)).toEqual({ kind: 'pause' })
    expect(parseQuickCommand(' /resume ', global, now)).toEqual({ kind: 'resume' })
    expect(parseQuickCommand('/pause', task, now)).toEqual({ kind: 'pause_task', task: 'T-1' })
    expect(parseQuickCommand('/resume', task, now)).toEqual({ kind: 'resume_task', task: 'T-1' })
    expect(parseQuickCommand('/pause', module, now).kind).toBe('error')
    expect(parseQuickCommand('/pause 现在', global, now).kind).toBe('error')
  })

  it('retries only a selected task', () => {
    expect(parseQuickCommand('/retry', task, now)).toEqual({ kind: 'retry', task: 'T-1' })
    expect(parseQuickCommand('/retry', global, now).kind).toBe('error')
    expect(parseQuickCommand('/retry', module, now).kind).toBe('error')
    expect(parseQuickCommand('/retry 快点', task, now).kind).toBe('error')
  })

  it('asks the node for a report due in a day', () => {
    const due = rfc3339Local(new Date(2026, 9, 25, 14, 30))
    expect(parseQuickCommand('/report', task, now)).toEqual({ kind: 'report', target: 'T-1', text: '请汇报当前进展', due })
    expect(parseQuickCommand('/report 卡在哪了？', module, now)).toEqual({ kind: 'report', target: 'A', text: '卡在哪了？', due })
    expect(parseQuickCommand('/report', global, now).kind).toBe('error')
  })

  it('sets a temporary rule only with an expiry inside thirty days', () => {
    expect(parseQuickCommand('/rule 今晚不要升级依赖 --until 2h', module, now))
      .toEqual({ kind: 'rule', scope: { kind: 'module', module: 'A' }, text: '今晚不要升级依赖', until: rfc3339Local(new Date(2026, 9, 24, 16, 30)) })
    expect(parseQuickCommand('/rule 先别合并 --until 2026-10-25 09:00', global, now))
      .toMatchObject({ kind: 'rule', until: rfc3339Local(new Date(2026, 9, 25, 9, 0)) })
    expect(parseQuickCommand('/rule 先别合并', global, now)).toMatchObject({ kind: 'error', message: expect.stringContaining('--until') })
    expect(parseQuickCommand('/rule 先别合并 --until 30d', global, now).kind).toBe('rule')
    expect(parseQuickCommand('/rule 先别合并 --until 31d', global, now)).toMatchObject({ kind: 'error', message: '临时规则最长 30 天' })
    expect(parseQuickCommand('/rule 先别合并 --until 2026-10-24 14:00', global, now).kind).toBe('error')
    expect(parseQuickCommand('/rule 先别合并 --until 今晚', global, now).kind).toBe('error')
    expect(parseQuickCommand(`/rule ${'长'.repeat(201)} --until 1h`, global, now).kind).toBe('error')
  })

  it('reads clock times as the next occurrence', () => {
    expect(parseUntil('18:00', now)).toEqual(new Date(2026, 9, 24, 18, 0))
    expect(parseUntil('09:00', now)).toEqual(new Date(2026, 9, 25, 9, 0))
    expect(parseUntil('25:00', now)).toBeNull()
    expect(parseUntil('2026-02-30 10:00', now)).toBeNull()
    expect(parseUntil('2026-10-25T01:00:00Z', now)).toEqual(new Date('2026-10-25T01:00:00Z'))
  })

  it('takes one assumption line apart from the answer', () => {
    const answer = { kind: 'answer' as const }
    expect(parseQuickCommand('用 CSV\n/assume 用户都用 Excel 打开', answer, now))
      .toEqual({ kind: 'answer', text: '用 CSV', assumption: '用户都用 Excel 打开' })
    expect(parseQuickCommand('用 CSV', answer, now)).toEqual({ kind: 'message', text: '用 CSV' })
    expect(parseQuickCommand('/assume 只有假设', answer, now)).toMatchObject({ kind: 'error' })
    expect(parseQuickCommand('用 CSV\n/assume', answer, now)).toMatchObject({ kind: 'error' })
    expect(parseQuickCommand('a\n/assume x\n/assume y', answer, now)).toMatchObject({ kind: 'error' })
    expect(parseQuickCommand('/assume 不在回答里', task, now)).toMatchObject({ kind: 'error' })
  })

  it('lists each command once with where it applies', () => {
    expect(QUICK_COMMANDS.map((command) => command.usage.split(' ')[0])).toEqual(['/pause', '/resume', '/retry', '/report', '/rule', '/assume'])
  })
})
