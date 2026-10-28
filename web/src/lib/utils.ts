import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

/** Class names, later ones winning Tailwind conflicts. shadcn-vue components expect it here. */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs))
}
