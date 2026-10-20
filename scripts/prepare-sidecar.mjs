import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, renameSync, chmodSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(dirname(fileURLToPath(import.meta.url)))
const release = process.argv.includes('--release')
const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, {
    cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, ...options,
  })
  if (result.error) throw result.error
  if (result.status !== 0) {
    if (result.stdout) process.stderr.write(result.stdout)
    if (result.stderr) process.stderr.write(result.stderr)
    throw new Error(`${command} exited with ${result.status}`)
  }
  return result.stdout
}
const host = run('rustc', ['-vV']).match(/^host: (.+)$/m)?.[1]
if (!host) throw new Error('rustc did not report its host triple')
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host
const args = ['build', '-p', 'towermd', '--locked', '--message-format=json-render-diagnostics', '-j', '2']
if (release) args.push('--release')
if (target !== host) args.push('--target', target)
const output = run('cargo', args, { stdio: ['ignore', 'pipe', 'inherit'] })
const messages = output.split('\n').filter(Boolean).map((line) => JSON.parse(line))
const executable = messages.findLast((message) =>
  message.reason === 'compiler-artifact' && message.target?.name === 'towermd' && message.executable,
)?.executable
if (!executable) throw new Error('cargo did not report a towermd executable')
const destination = join(root, 'src-tauri', 'binaries', `towermd-${target}${target.includes('windows') ? '.exe' : ''}`)
mkdirSync(dirname(destination), { recursive: true })
const temporary = `${destination}.${process.pid}.tmp`
copyFileSync(executable, temporary)
chmodSync(temporary, 0o755)
renameSync(temporary, destination)
console.log(`Prepared ${destination}`)
