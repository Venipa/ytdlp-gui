import LogoImage from '@renderer/assets/appIcon.png'
import { HTMLProps } from 'react'

export default function Logo(props: HTMLProps<HTMLImageElement>) {
  return <img src={LogoImage} alt="logo" {...props} />
}
