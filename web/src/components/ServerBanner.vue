<script setup lang="ts">
// Shown while the health check fails; it polls, so the banner clears once the server answers.
import { useHealth } from '@/api/health'

const { isError } = useHealth()
</script>

<template>
  <div
    v-if="isError"
    role="alert"
    class="border-b border-destructive/30 bg-destructive/10 px-4 py-2 text-sm text-destructive lg:px-6"
  >
    Can't reach the server. Changes won't save until it's back; retrying.
  </div>
</template>
