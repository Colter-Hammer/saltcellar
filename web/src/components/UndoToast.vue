<script setup lang="ts">
// A toast with Undo, for an action that has already happened (docs/spec/m2-bank-data.md#manual-entry:
// deleting a manual row). It sits above the tab bar on phones and in the corner on desktops, and
// goes after 8 seconds, or on Dismiss *(agent)*.
import { onBeforeUnmount, onMounted } from 'vue'

const props = withDefaults(defineProps<{ message: string; seconds?: number }>(), { seconds: 8 })
const emit = defineEmits<{ undo: []; dismiss: [] }>()

let timer: ReturnType<typeof globalThis.setTimeout> | undefined
onMounted(() => {
  timer = globalThis.setTimeout(() => {
    emit('dismiss')
  }, props.seconds * 1000)
})
onBeforeUnmount(() => {
  globalThis.clearTimeout(timer)
})
</script>

<template>
  <div
    role="status"
    class="fixed inset-x-4 bottom-20 z-50 flex items-center gap-3 rounded-xl border bg-background px-4 py-3 shadow-lg lg:inset-x-auto lg:right-6 lg:bottom-6"
  >
    <span class="flex-1">{{ message }}</span>
    <button type="button" class="rounded-md border px-3 py-1 font-medium" @click="emit('undo')">
      Undo
    </button>
    <button
      type="button"
      class="text-muted-foreground"
      aria-label="Dismiss"
      @click="emit('dismiss')"
    >
      ✕
    </button>
  </div>
</template>
