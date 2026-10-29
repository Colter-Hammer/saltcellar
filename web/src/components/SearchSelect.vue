<script setup lang="ts">
// A labelled, searchable pick-one list (Reka UI's Combobox): tap to see every option, type to
// narrow them by any part of their label. Only an option can be chosen, so the value is always one
// of `options`. Phones get a real list, which `<datalist>` doesn't give them.
import {
  ComboboxAnchor,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxRoot,
  ComboboxViewport,
} from 'reka-ui'
import { computed, ref } from 'vue'

export interface Option {
  value: string
  label: string
}

const props = defineProps<{
  name: string
  label: string
  options: readonly Option[]
  hint?: string | undefined
  error?: string | undefined
}>()
const value = defineModel<string>({ required: true })

/** What's typed since the list opened; empty shows every option. */
const query = ref('')
const shown = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean)
  return props.options.filter((option) => {
    const label = option.label.toLowerCase()
    return words.every((word) => label.includes(word))
  })
})

function labelOf(chosen: unknown): string {
  return props.options.find((option) => option.value === chosen)?.label ?? ''
}

function onOpen(open: boolean) {
  if (open) query.value = ''
}

const describedBy = computed(
  () =>
    [props.hint && `${props.name}-hint`, props.error && `${props.name}-error`]
      .filter(Boolean)
      .join(' ') || undefined,
)
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <label :for="name" class="text-sm font-medium">{{ label }}</label>
    <p v-if="hint" :id="`${name}-hint`" class="text-sm text-muted-foreground">{{ hint }}</p>
    <ComboboxRoot
      v-model="value"
      :name="name"
      ignore-filter
      open-on-click
      reset-search-term-on-blur
      @update:open="onOpen"
    >
      <ComboboxAnchor class="flex">
        <ComboboxInput
          :id="name"
          :display-value="labelOf"
          :aria-invalid="error ? true : undefined"
          :aria-describedby="describedBy"
          autocomplete="off"
          class="w-full rounded-md border px-3 py-2"
          @input="query = ($event.target as HTMLInputElement).value"
        />
      </ComboboxAnchor>
      <ComboboxContent class="mt-1 rounded-md border bg-background shadow-sm">
        <ComboboxViewport class="max-h-64 overflow-y-auto p-1">
          <ComboboxEmpty class="px-2 py-1.5 text-sm text-muted-foreground">
            Nothing matches.
          </ComboboxEmpty>
          <ComboboxItem
            v-for="option in shown"
            :key="option.value"
            :value="option.value"
            class="cursor-pointer rounded px-2 py-1.5 text-sm data-[highlighted]:bg-accent"
          >
            {{ option.label }}
          </ComboboxItem>
        </ComboboxViewport>
      </ComboboxContent>
    </ComboboxRoot>
    <p v-if="error" :id="`${name}-error`" class="text-sm text-destructive">{{ error }}</p>
  </div>
</template>
