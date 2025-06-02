import { HTMLProps } from "react";

export default function Image(props: HTMLProps<HTMLImageElement>) {
	return <img alt='logo' {...props} />;
}
