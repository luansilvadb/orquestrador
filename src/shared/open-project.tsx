import { Center } from '@lobehub/ui';
import { Button, Text } from '@lobehub/ui/base-ui';

export function OpenProjectPrompt({ onOpen, description = 'Abra uma pasta para usar este painel.' }: {
  onOpen: () => void;
  description?: string;
}) {
  return (
    <Center gap={8} className="h-full p-4">
      <Text as="p" fontSize={13} type="secondary" align="center">{description}</Text>
      <Button type="primary" size="small" onClick={onOpen}>Abrir pasta</Button>
    </Center>
  );
}
