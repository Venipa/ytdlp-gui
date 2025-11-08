import { sn } from "@renderer/lib/utils";
import { MouseEvent, forwardRef, useCallback, useEffect, useState } from "react";
import { Button, ButtonProps } from "./button";
import { Spinner } from "./spinner";
interface ButtonLoadingProps extends ButtonProps {
	loading?: boolean;
	onClickWithLoading?: (ev: MouseEvent<HTMLButtonElement>) => Promise<void>;
	fixWidth?: boolean;
}
export default forwardRef(function ButtonLoading({ children, loading: refLoading, onClickWithLoading, fixWidth, ...props }: ButtonLoadingProps, ref: any) {
	const [loading, setLoading] = useState<boolean>(() => !!refLoading);
	const [fixedWidth, setFixedWidth] = useState<number>();
	const handleClick = useCallback(
		(ev: MouseEvent<HTMLButtonElement>) => {
			if (fixWidth) setFixedWidth(ev.currentTarget.clientWidth);
			if (onClickWithLoading) {
				setLoading(true);
				onClickWithLoading(ev).finally(() => setLoading(false));
			}
		},
		[props.onClick],
	);
	useEffect(() => {
		setLoading(!!refLoading);
	}, [refLoading]);
	return (
		<Button
			{...props}
			onClick={props.onClick ?? handleClick}
			disabled={loading || props.disabled}
			variant={loading ? "outline" : props.variant}
			ref={ref}
			style={sn(!!fixedWidth && { width: fixedWidth })}>
			{loading ? <Spinner /> : children}
		</Button>
	);
});
