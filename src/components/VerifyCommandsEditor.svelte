<script lang="ts">
  import type { VerifyCommand } from '../lib/draft';
  let { commands, busy = false, onchange }: {
    commands: VerifyCommand[]; busy?: boolean; onchange: (commands: VerifyCommand[]) => void;
  } = $props();
  function update(index: number, value: VerifyCommand) {
    onchange(commands.map((command, i) => i === index ? value : command));
  }
</script>

<div class="verify-commands">
  {#each commands as command, index}
    <fieldset>
      <legend>验证命令 {index + 1}</legend>
      <label>工作目录<input value={command.cwd} required disabled={busy} oninput={event => update(index, {...command, cwd: event.currentTarget.value})}/></label>
      {#each command.argv as arg, argIndex}
        <div class="draft-fields">
          <label>{argIndex === 0 ? '程序' : `参数 ${argIndex}`}<input value={arg} required={argIndex === 0} disabled={busy} oninput={event => update(index, {...command, argv: command.argv.map((value, i) => i === argIndex ? event.currentTarget.value : value)})}/></label>
          {#if argIndex > 0}<button class="secondary" type="button" disabled={busy} onclick={() => update(index, {...command, argv: command.argv.filter((_, i) => i !== argIndex)})}>移除参数 {argIndex}</button>{/if}
        </div>
      {/each}
      <button class="secondary" type="button" disabled={busy} onclick={() => update(index, {...command, argv: [...command.argv, '']})}>添加参数</button>
      <button class="secondary" type="button" disabled={busy} onclick={() => onchange(commands.filter((_, i) => i !== index))}>移除命令 {index + 1}</button>
    </fieldset>
  {/each}
  <button class="secondary" type="button" disabled={busy} onclick={() => onchange([...commands, {cwd: '.', argv: ['']}])}>添加验证命令</button>
  <p class="status">每个参数单独填写，包含空格的参数无需加引号；保存时不执行命令。空列表表示未配置验证命令。</p>
</div>
