// // 'use client';

// import { Card, SparkAreaChart } from '@tremor/react'

// function classNames(...classes) {
//   return classes.filter(Boolean).join(' ')
// }

// const data = [
//   {
//     date: 'Nov 24, 2023',
//     GOOG: 156.2,
//     AMZN: 68.5,
//     SPOT: 71.8,
//     MSFT: 205.3,
//     TSLA: 1050.6
//   },
//   {
//     date: 'Nov 25, 2023',
//     GOOG: 152.5,
//     AMZN: 69.3,
//     SPOT: 67.2,
//     MSFT: 223.1,
//     TSLA: 945.8
//   },
//   {
//     date: 'Nov 26, 2023',
//     GOOG: 148.7,
//     AMZN: 69.8,
//     SPOT: 61.5,
//     MSFT: 240.9,
//     TSLA: 839.4
//   },
//   {
//     date: 'Nov 27, 2023',
//     GOOG: 155.3,
//     AMZN: 73.5,
//     SPOT: 57.9,
//     MSFT: 254.7,
//     TSLA: 830.2
//   },
//   {
//     date: 'Nov 28, 2023',
//     GOOG: 160.1,
//     AMZN: 75.2,
//     SPOT: 59.6,
//     MSFT: 308.5,
//     TSLA: 845.7
//   },
//   {
//     date: 'Nov 29, 2023',
//     GOOG: 175.6,
//     AMZN: 89.2,
//     SPOT: 57.3,
//     MSFT: 341.4,
//     TSLA: 950.2
//   },
//   {
//     date: 'Nov 30, 2023',
//     GOOG: 180.2,
//     AMZN: 92.7,
//     SPOT: 59.8,
//     MSFT: 378.1,
//     TSLA: 995.9
//   },
//   {
//     date: 'Dec 01, 2023',
//     GOOG: 185.5,
//     AMZN: 95.3,
//     SPOT: 62.4,
//     MSFT: 320.3,
//     TSLA: 1060.4
//   },
//   {
//     date: 'Dec 02, 2023',
//     GOOG: 182.3,
//     AMZN: 93.8,
//     SPOT: 60.7,
//     MSFT: 356.5,
//     TSLA: 965.8
//   },
//   {
//     date: 'Dec 03, 2023',
//     GOOG: 180.7,
//     AMZN: 91.6,
//     SPOT: 58.9,
//     MSFT: 340.7,
//     TSLA: 970.3
//   },
//   {
//     date: 'Dec 04, 2023',
//     GOOG: 178.5,
//     AMZN: 89.7,
//     SPOT: 56.2,
//     MSFT: 365.9,
//     TSLA: 975.9
//   },
//   {
//     date: 'Dec 05, 2023',
//     GOOG: 176.2,
//     AMZN: 87.5,
//     SPOT: 54.8,
//     MSFT: 375.1,
//     TSLA: 964.6
//   },
//   {
//     date: 'Dec 06, 2023',
//     GOOG: 174.8,
//     AMZN: 85.3,
//     SPOT: 53.4,
//     MSFT: 340.3,
//     TSLA: 960.4
//   },
//   {
//     date: 'Dec 07, 2023',
//     GOOG: 178.0,
//     AMZN: 88.2,
//     SPOT: 55.2,
//     MSFT: 335.5,
//     TSLA: 955.3
//   },
//   {
//     date: 'Dec 08, 2023',
//     GOOG: 180.6,
//     AMZN: 90.5,
//     SPOT: 56.8,
//     MSFT: 310.7,
//     TSLA: 950.3
//   },
//   {
//     date: 'Dec 09, 2023',
//     GOOG: 182.4,
//     AMZN: 92.8,
//     SPOT: 58.4,
//     MSFT: 300.9,
//     TSLA: 845.4
//   },
//   {
//     date: 'Dec 10, 2023',
//     GOOG: 179.7,
//     AMZN: 90.2,
//     SPOT: 57.0,
//     MSFT: 290.1,
//     TSLA: 1011.6
//   },
//   {
//     date: 'Dec 11, 2023',
//     GOOG: 154.2,
//     AMZN: 88.7,
//     SPOT: 55.6,
//     MSFT: 291.3,
//     TSLA: 1017.9
//   },
//   {
//     date: 'Dec 12, 2023',
//     GOOG: 151.9,
//     AMZN: 86.5,
//     SPOT: 53.9,
//     MSFT: 293.5,
//     TSLA: 940.3
//   },
//   {
//     date: 'Dec 13, 2023',
//     GOOG: 149.3,
//     AMZN: 83.7,
//     SPOT: 52.1,
//     MSFT: 301.7,
//     TSLA: 900.8
//   },
//   {
//     date: 'Dec 14, 2023',
//     GOOG: 148.8,
//     AMZN: 81.4,
//     SPOT: 50.5,
//     MSFT: 321.9,
//     TSLA: 780.4
//   },
//   {
//     date: 'Dec 15, 2023',
//     GOOG: 145.5,
//     AMZN: 79.8,
//     SPOT: 48.9,
//     MSFT: 328.1,
//     TSLA: 765.1
//   },
//   {
//     date: 'Dec 16, 2023',
//     GOOG: 140.2,
//     AMZN: 84.5,
//     SPOT: 50.2,
//     MSFT: 331.3,
//     TSLA: 745.9
//   },
//   {
//     date: 'Dec 17, 2023',
//     GOOG: 143.8,
//     AMZN: 82.1,
//     SPOT: 49.6,
//     MSFT: 373.5,
//     TSLA: 741.8
//   },
//   {
//     date: 'Dec 18, 2023',
//     GOOG: 157.5,
//     AMZN: 86.9,
//     SPOT: 51.3,
//     MSFT: 381.7,
//     TSLA: 739.8
//   }
// ]

// const summary = [
//   {
//     ticker: 'AMZN',
//     description: 'Amazon',
//     value: '$86.9',
//     change: '+0.92%',
//     changeType: 'positive'
//   },
//   {
//     ticker: 'TSLA',
//     description: 'Tesla, Inc.',
//     value: '$739.8',
//     change: '-2.12%',
//     changeType: 'negative'
//   },
//   {
//     ticker: 'GOOG',
//     description: 'Alphabet, Inc',
//     value: '$157.5',
//     change: '+0.38%',
//     changeType: 'positive'
//   },
//   {
//     ticker: 'SPOT',
//     description: 'Spotify',
//     value: '$51.3',
//     change: '−0.25%',
//     changeType: 'negative'
//   },
//   {
//     ticker: 'MSFT',
//     description: 'Microsoft',
//     value: '$381.7',
//     change: '+2.45%',
//     changeType: 'positive'
//   }
// ]
// interface Props {}
// export default function PercentChardCard({}: Props) {
//   return (
//     <>
//       <dl className="space-y-6 sm:mx-auto sm:max-w-md">
//         {summary.map((stock) => (
//           <Card key={stock.ticker} className="flex items-center justify-between space-x-4 p-4">
//             <div className="truncate">
//               <dt className="truncate text-tremor-default font-medium text-tremor-content-strong dark:text-dark-tremor-content-strong">
//                 {stock.ticker}
//               </dt>
//               <dl className="truncate text-tremor-label text-tremor-content dark:text-dark-tremor-content">
//                 {stock.description}
//               </dl>
//             </div>
//             <div className="flex items-center space-x-3">
//               <SparkAreaChart
//                 data={data}
//                 index="date"
//                 categories={[stock.ticker]}
//                 showGradient={true}
//                 colors={stock.changeType === 'positive' ? ['emerald'] : ['red']}
//                 className="h-8 w-24 flex-none sm:w-28"
//               />
//               <div
//                 className={classNames(
//                   stock.changeType === 'positive'
//                     ? 'text-emerald-700 dark:text-emerald-500'
//                     : 'text-red-700 dark:text-red-500',
//                   'flex w-28 items-center justify-end space-x-2 sm:w-32'
//                 )}
//               >
//                 <dd className="text-tremor-default font-medium">{stock.value}</dd>
//                 <dd
//                   className={classNames(
//                     stock.changeType === 'positive'
//                       ? 'bg-emerald-100 dark:bg-emerald-400/10'
//                       : 'bg-red-100 dark:bg-red-400/10',
//                     'rounded px-1.5 py-1 text-tremor-label font-medium tabular-nums'
//                   )}
//                 >
//                   {stock.change}
//                 </dd>
//               </div>
//             </div>
//           </Card>
//         ))}
//       </dl>
//     </>
//   )
// }
