<script lang="ts">
  import type { CanvasNode, ProjectView } from '../lib/api'
  let { view, onOpen, fitVersion }: { view: ProjectView; fitVersion?: number; onOpen: (node: CanvasNode) => void } = $props()
</script>

<section aria-label="项目画布" data-fit-version={fitVersion}>
  {#each view.nodes as node}
    <button onclick={() => onOpen(node)}>查看 {node.title}</button>
  {/each}
</section>
