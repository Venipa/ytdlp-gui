import { useCallback, useEffect, useMemo, useState } from 'react'

export default function usePromise<T>(promise: () => Promise<T>): {
  loading: boolean
  result: T | null
  error: any
} {
  const [loading, setLoading] = useState(true)
  const [result, setResult] = useState<T | null>(null)
  const [error, setError] = useState(null)
  const revalidate = useCallback(() => {
    setLoading(true)
    promise()
      .then((result) => {
        setResult(result)
      })
      .catch((error) => {
        setError(error)
      })
      .finally(() => setLoading(false))
  }, [promise])

  useEffect(() => {
    revalidate()
  }, [promise])

  return useMemo(
    () => ({ loading, result, error, revalidate }),
    [loading, result, error, revalidate]
  )
}
