export function List(props) @{
	const items = props.items
	<ul>
		@for (const item of items) {
			const label = {
				text: item.name,
			}
			<li>{label.text}</li>
		}
	</ul>
}
