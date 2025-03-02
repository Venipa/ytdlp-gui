import { HTMLProps } from 'react'
import LogoImage from '~/build/icon_1024x1024.png'

export default function Logo(props: HTMLProps<HTMLImageElement>) {
  return <div className="flex-shrink-0"><img src={LogoImage} alt="logo" {...props} /></div>
}
