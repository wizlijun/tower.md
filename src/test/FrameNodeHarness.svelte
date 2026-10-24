<script lang="ts">
  import { SvelteFlow, SvelteFlowProvider } from '@xyflow/svelte'
  import FrameNode from '../components/FrameNode.svelte'
  import type { FrameNode as FrameFlowNode } from '../lib/frames'

  // Renders one frame node the way the canvas does, for component tests.
  let { node }: { node: FrameFlowNode } = $props()
  const nodeTypes = { frame: FrameNode }
</script>

<SvelteFlowProvider>
  <div style="width: 1200px; height: 800px;">
    <SvelteFlow nodes={[node]} edges={[]} {nodeTypes} />
  </div>
</SvelteFlowProvider>
