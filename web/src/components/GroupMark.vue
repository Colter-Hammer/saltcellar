<script setup lang="ts">
// A group's color dot and icon, beside the name of the group or of one of its categories
// (docs/spec/visual-system.md#group-color-and-icon). Decorative: the name beside it says the same.
// Shows nothing for no category (split, transfer, uncategorized). What it shows comes from
// src/lib/groups.ts, never from here.
import { computed } from 'vue'
import { useCategories } from '@/api/categories'
import { groupLooks } from '@/lib/groups'

const props = defineProps<{
  /** The group's id; or give `category`. */
  group?: number | null | undefined
  /** A category's id, to show its group's look. */
  category?: number | null | undefined
  /** Only the dot, where the group's icon is already in view *(agent)*. */
  dotOnly?: boolean
}>()

const categories = useCategories()
const look = computed(() => {
  const looks = groupLooks(categories.data.value?.groups ?? [])
  if (props.group != null) return looks.byGroup.get(props.group)
  if (props.category != null) return looks.byCategory.get(props.category)
  return undefined
})
</script>

<template>
  <span
    v-if="look"
    class="inline-flex shrink-0 items-center gap-1"
    aria-hidden="true"
    data-testid="group-mark"
    :data-icon="look.icon.name"
  >
    <span class="size-2 rounded-full" :style="{ backgroundColor: look.color }" />
    <component :is="look.icon.icon" v-if="!dotOnly" class="size-4 text-muted-foreground" />
  </span>
</template>
