<script setup lang="ts">
// Logs out of this browser, forgets what it had loaded, and leaves on the log-in page.
import { useQueryClient } from '@tanstack/vue-query'
import { LogOut } from '@lucide/vue'
import { useRouter } from 'vue-router'
import { logOut } from '@/api/session'

const router = useRouter()
const queryClient = useQueryClient()

async function leave() {
  await logOut()
  queryClient.clear()
  await router.push('/login')
}
</script>

<template>
  <button type="button" class="flex w-full items-center gap-2 text-left" @click="leave">
    <LogOut class="size-4 text-muted-foreground" aria-hidden="true" />
    Log out
  </button>
</template>
