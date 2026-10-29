<script setup lang="ts">
// Settings: one entry per page of settings. Categories and Imports join Profile in later issues.
import { UserRound } from '@lucide/vue'

const pages = [{ to: '/settings/profile', label: 'Profile', icon: UserRound }]
</script>

<template>
  <ul class="m-4 divide-y rounded-xl border lg:mx-6 lg:max-w-md">
    <li v-for="page in pages" :key="page.to">
      <RouterLink :to="page.to" class="flex items-center gap-3 p-3">
        <component :is="page.icon" class="size-4 text-muted-foreground" aria-hidden="true" />
        {{ page.label }}
      </RouterLink>
    </li>
  </ul>
</template>
