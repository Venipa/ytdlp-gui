import { useQueryClient } from "@tanstack/react-query";

export const useGetCachedQueryData = <T = any>(key: string) => {
	const queryClient = useQueryClient();
	const data = queryClient.getQueryData([key]);
	return data as T;
};
