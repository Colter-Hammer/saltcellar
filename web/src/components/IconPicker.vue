<script setup lang="ts">
// Pick a group's icon from the curated set (docs/spec/visual-system.md#group-color-and-icon): a
// row of radio buttons, each an icon with its name for screen readers. The radio itself covers its
// tile, unseen, so a tap anywhere on the tile picks it.
import { GROUP_ICONS } from '@/lib/groups'

defineProps<{
  /** The radio group's name; unique on the page. */
  name: string
}>()
const value = defineModel<string>({ required: true })
</script>

<template>
  <fieldset class="flex flex-col gap-1">
    <legend class="mb-1 text-sm font-medium">Icon</legend>
    <div class="grid grid-cols-[repeat(auto-fill,minmax(var(--row-touch),1fr))] gap-1">
      <label
        v-for="option in GROUP_ICONS"
        :key="option.name"
        :title="option.label"
        class="relative flex size-row-touch cursor-pointer items-center justify-center rounded-md border text-muted-foreground has-checked:border-primary has-checked:bg-muted has-checked:text-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
      >
        <input
          v-model="value"
          type="radio"
          :name="name"
          :value="option.name"
          class="absolute inset-0 cursor-pointer opacity-0"
        />
        <component :is="option.icon" class="size-5" aria-hidden="true" />
        <span class="sr-only">{{ option.label }}</span>
      </label>
    </div>
  </fieldset>
</template>
