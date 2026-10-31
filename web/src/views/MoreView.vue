<script setup lang="ts">
// Phones only reach these through More; the desktop sidebar lists them directly.
import { computed } from 'vue'
import { useUndecided } from '@/api/undecided'
import LogOutButton from '@/components/LogOutButton.vue'
import { moreItems } from '@/lib/nav'

const undecided = useUndecided()
const items = computed(() => moreItems((undecided.data.value?.length ?? 0) > 0))
</script>

<template>
  <ul class="m-4 divide-y rounded-xl border lg:mx-6 lg:max-w-md">
    <li v-for="item in items" :key="item.to">
      <RouterLink :to="item.to" class="flex items-center gap-3 p-3">
        <component :is="item.icon" class="size-4 text-muted-foreground" aria-hidden="true" />
        {{ item.label }}
      </RouterLink>
    </li>
    <li><LogOutButton class="gap-3 p-3" /></li>
  </ul>
</template>
