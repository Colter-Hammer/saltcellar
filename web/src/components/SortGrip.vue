<script setup lang="ts">
// The handle that lifts a row of a sortable list (src/lib/sortable.ts): a pointer drags it, the
// Up / Down keys move the row. `touch-none` keeps a finger on the handle from scrolling the page,
// so the rest of the row still scrolls. Wire `@pointerdown` and `@keydown` to the list's sortable.
import { GripVertical } from '@lucide/vue'

defineProps<{ label: string }>()
</script>

<template>
  <button
    type="button"
    :aria-label="label"
    aria-keyshortcuts="ArrowUp ArrowDown"
    title="Drag, or press Up or Down"
    class="inline-flex shrink-0 cursor-grab touch-none items-center justify-center rounded-md text-muted-foreground hover:text-foreground focus-visible:outline-2 max-lg:min-w-row-touch lg:size-6"
  >
    <GripVertical class="size-4" aria-hidden="true" />
  </button>
</template>
